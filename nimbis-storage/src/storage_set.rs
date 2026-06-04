use bytes::Buf;
use bytes::Bytes;
use nimbis_macros::storage_lock;
use slatedb::WriteBatch;
use slatedb::config::PutOptions;

use crate::error::StorageError;
use crate::segment::Segment;
use crate::set::member_key::SetMemberKey;
use crate::storage::Storage;
use crate::string::meta::MetaKey;
use crate::string::meta::SetMetaValue;
use crate::utils::collection_version_prefix;

impl Storage {
	#[storage_lock(write, key)]
	#[fastrace::trace]
	pub async fn sadd(&self, key: Bytes, members: Vec<Bytes>) -> Result<u64, StorageError> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = Segment::Meta.wrap(meta_key.encode());
		let put_opts = PutOptions::default();

		let (mut meta_val, meta_missing) = match self.get_meta::<SetMetaValue>(&key).await? {
			Some(meta) => (meta, false),
			None => (SetMetaValue::new(self.next_version(), 0), true),
		};

		// Deduplicate members, keeping the first occurrence
		let mut unique_members = std::collections::HashSet::new();
		let members: Vec<_> = members
			.into_iter()
			.filter(|m| unique_members.insert(m.clone()))
			.collect();

		let mut added_count = 0;
		let mut batch = WriteBatch::new();

		for member in members {
			let member_key = SetMemberKey::new(key.clone(), meta_val.version, member);
			let encoded_member_key = Segment::Set.wrap(member_key.encode());
			let exists = if meta_missing {
				false
			} else {
				self.db
					.get_key_value(encoded_member_key.clone())
					.await?
					.is_some()
			};

			if !exists {
				batch.put_with_options(
					encoded_member_key,
					Bytes::new(), // value is empty for set members
					&put_opts,
				);
				added_count += 1;
			}
		}

		if added_count > 0 {
			meta_val.len += added_count;

			let put_opts = Storage::meta_put_opts(&meta_val);

			batch.put_with_options(meta_encoded_key, meta_val.encode(), &put_opts);
			self.write_batch(batch).await?;
		}

		Ok(added_count)
	}

	#[storage_lock(read, key)]
	#[fastrace::trace]
	pub async fn smembers(&self, key: Bytes) -> Result<Vec<Bytes>, StorageError> {
		let Some(meta_val) = self.get_meta::<SetMetaValue>(&key).await? else {
			return Ok(Vec::new());
		};

		// Construct prefix: len(user_key) + user_key + version
		let prefix = Segment::Set.wrap(collection_version_prefix(&key, meta_val.version));

		let range = prefix.clone()..;
		let mut stream = self.db.scan(range).await?;
		let mut members = Vec::new();

		while let Some(kv) = stream.next().await? {
			let k = kv.key;
			if !k.starts_with(&prefix) {
				break;
			}
			// Parse member: prefix (key_len+key+version) + member_len(u32) + member
			let suffix = &k[prefix.len()..];
			if suffix.len() < 4 {
				continue;
			}

			let mut buf = suffix;
			let member_len = buf.get_u32() as usize;

			if buf.len() != member_len {
				continue;
			}

			let member = Bytes::copy_from_slice(buf);
			members.push(member);
		}

		Ok(members)
	}

	#[storage_lock(read, key)]
	#[fastrace::trace]
	pub async fn sismember(&self, key: Bytes, member: Bytes) -> Result<bool, StorageError> {
		let Some(meta_val) = self.get_meta::<SetMetaValue>(&key).await? else {
			return Ok(false);
		};

		let member_key = SetMemberKey::new(key, meta_val.version, member);
		let found = self
			.db
			.get_key_value(Segment::Set.wrap(member_key.encode()))
			.await?
			.is_some();
		Ok(found)
	}

	#[storage_lock(write, key)]
	#[fastrace::trace]
	pub async fn srem(&self, key: Bytes, members: Vec<Bytes>) -> Result<u64, StorageError> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = Segment::Meta.wrap(meta_key.encode());

		let mut meta_val = match self.get_meta::<SetMetaValue>(&key).await? {
			Some(val) => val,
			None => return Ok(0),
		};

		let mut removed_count = 0;
		let mut batch = WriteBatch::new();

		for member in members {
			let member_key = SetMemberKey::new(key.clone(), meta_val.version, member);
			let encoded_key = Segment::Set.wrap(member_key.encode());
			let exists = self.db.get_key_value(encoded_key.clone()).await?.is_some();

			if exists {
				batch.delete(encoded_key);
				removed_count += 1;
			}
		}

		if removed_count > 0 {
			meta_val.len -= removed_count;
			if meta_val.len == 0 {
				batch.delete(meta_encoded_key);
			} else {
				let put_opts = Storage::meta_put_opts(&meta_val);
				batch.put_with_options(meta_encoded_key, meta_val.encode(), &put_opts);
			}
			self.write_batch(batch).await?;
		}

		Ok(removed_count)
	}

	#[storage_lock(read, key)]
	#[fastrace::trace]
	pub async fn scard(&self, key: Bytes) -> Result<u64, StorageError> {
		if let Some(meta_val) = self.get_meta::<SetMetaValue>(&key).await? {
			Ok(meta_val.len)
		} else {
			Ok(0)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::string::meta::SetMetaValue;

	async fn get_storage() -> (Storage, std::path::PathBuf) {
		let timestamp = ulid::Ulid::new().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_test_set_{}", timestamp));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		(storage, path)
	}

	#[tokio::test]
	async fn test_sadd_smembers() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("myset");
		let m1 = Bytes::from("m1");
		let m2 = Bytes::from("m2");

		let added = storage.sadd(key.clone(), vec![m1.clone()]).await.unwrap();
		assert_eq!(added, 1);

		let added = storage
			.sadd(key.clone(), vec![m1.clone(), m2.clone()])
			.await
			.unwrap();
		assert_eq!(added, 1); // Only m2 is new

		let members = storage.smembers(key.clone()).await.unwrap();
		assert_eq!(members.len(), 2);
		assert!(members.contains(&m1));
		assert!(members.contains(&m2));

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_sismember() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("myset");
		let m1 = Bytes::from("m1");

		storage.sadd(key.clone(), vec![m1.clone()]).await.unwrap();

		assert!(storage.sismember(key.clone(), m1.clone()).await.unwrap());
		assert!(
			!storage
				.sismember(key.clone(), Bytes::from("missing"))
				.await
				.unwrap()
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_srem() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("myset");
		let m1 = Bytes::from("m1");
		let m2 = Bytes::from("m2");

		storage
			.sadd(key.clone(), vec![m1.clone(), m2.clone()])
			.await
			.unwrap();

		let removed = storage.srem(key.clone(), vec![m1.clone()]).await.unwrap();
		assert_eq!(removed, 1);

		let members = storage.smembers(key.clone()).await.unwrap();
		assert_eq!(members.len(), 1);
		assert!(members.contains(&m2));

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_scard() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("myset");
		let m1 = Bytes::from("m1");

		assert_eq!(storage.scard(key.clone()).await.unwrap(), 0);

		storage.sadd(key.clone(), vec![m1.clone()]).await.unwrap();
		assert_eq!(storage.scard(key.clone()).await.unwrap(), 1);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_set_version_init_stable_and_recreate() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("set_version_lifecycle");
		let m1 = Bytes::from("m1");
		let m2 = Bytes::from("m2");

		let added = storage.sadd(key.clone(), vec![m1.clone()]).await.unwrap();
		assert_eq!(added, 1);

		let version_v1 = storage
			.get_meta::<SetMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;

		let added = storage.sadd(key.clone(), vec![m1.clone()]).await.unwrap();
		assert_eq!(added, 0);

		let version_after_dup = storage
			.get_meta::<SetMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;
		assert_eq!(version_after_dup, version_v1);

		let added = storage.sadd(key.clone(), vec![m2.clone()]).await.unwrap();
		assert_eq!(added, 1);

		let version_after_update = storage
			.get_meta::<SetMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;
		assert_eq!(version_after_update, version_v1);

		let deleted = storage.del([key.clone()]).await.unwrap();
		assert_eq!(deleted, 1);

		let added = storage.sadd(key.clone(), vec![m1.clone()]).await.unwrap();
		assert_eq!(added, 1);

		let version_v2 = storage
			.get_meta::<SetMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;
		assert!(version_v2 > version_v1);

		let members = storage.smembers(key.clone()).await.unwrap();
		assert_eq!(members, vec![m1]);

		let _ = std::fs::remove_dir_all(path);
	}
}
