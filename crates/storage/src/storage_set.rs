use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;

use crate::error::StorageError;
use crate::set::member_key::SetMemberKey;
use crate::storage::Storage;
use crate::string::meta::MetaKey;
use crate::string::meta::SetMetaValue;

impl Storage {
	pub async fn sadd(&self, key: Bytes, members: Vec<Bytes>) -> Result<u64, StorageError> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = meta_key.encode();
		let write_opts = WriteOptions {
			await_durable: false,
		};
		let put_opts = PutOptions::default();

		let (mut meta_val, meta_missing) = match self.get_meta::<SetMetaValue>(&key).await? {
			Some(meta) => (meta, false),
			None => (SetMetaValue::new(0, 0), true),
		};

		let mut added_count = 0;
		let mut first_added_seq: Option<u64> = None;

		for member in members {
			let member_key = SetMemberKey::new(key.clone(), member);
			let encoded_member_key = member_key.encode();
			let exists = if meta_missing {
				false
			} else {
				self.get_with_meta(&self.set_db, encoded_member_key.clone())
					.await?
					.is_some_and(|entry| entry.seq >= meta_val.version)
			};

			if !exists {
				let wh = self
					.set_db
					.put_with_options(
						encoded_member_key,
						Bytes::new(), // value is empty for set members
						&put_opts,
						&write_opts,
					)
					.await?;
				if meta_missing && first_added_seq.is_none() {
					first_added_seq = Some(wh.seqnum());
				}
				added_count += 1;
			}
		}

		if added_count > 0 {
			if meta_missing {
				meta_val.version = first_added_seq.ok_or_else(|| StorageError::DataInconsistency {
					message: "missing first new set member seq after write".to_string(),
				})?;
			}
			meta_val.len += added_count;

			let put_opts = PutOptions::default();

			self.string_db
				.put_with_options(meta_encoded_key, meta_val.encode(), &put_opts, &write_opts)
				.await?;
		}

		Ok(added_count)
	}

	pub async fn smembers(&self, key: Bytes) -> Result<Vec<Bytes>, StorageError> {
		let Some(meta_val) = self.get_meta::<SetMetaValue>(&key).await? else {
			return Ok(Vec::new());
		};

		// Construct prefix: len(user_key) + user_key
		let mut prefix = BytesMut::with_capacity(2 + key.len());
		prefix.put_u16(key.len() as u16);
		prefix.extend_from_slice(&key);
		let prefix = prefix.freeze();

		let range = prefix.clone()..;
		let mut stream = self.set_db.scan(range).await?;
		let mut members = Vec::new();

		while let Some(kv) = stream.next().await? {
			let k = kv.key;
			if kv.seq < meta_val.version {
				continue;
			}
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

	pub async fn sismember(&self, key: Bytes, member: Bytes) -> Result<bool, StorageError> {
		let Some(meta_val) = self.get_meta::<SetMetaValue>(&key).await? else {
			return Ok(false);
		};

		let member_key = SetMemberKey::new(key, member);
		let found = self
			.get_with_meta(&self.set_db, member_key.encode())
			.await?
			.is_some_and(|entry| entry.seq >= meta_val.version);
		Ok(found)
	}

	pub async fn srem(&self, key: Bytes, members: Vec<Bytes>) -> Result<u64, StorageError> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = meta_key.encode();

		let mut meta_val = match self.get_meta::<SetMetaValue>(&key).await? {
			Some(val) => val,
			None => return Ok(0),
		};

		let mut removed_count = 0;
		let write_opts = WriteOptions {
			await_durable: false,
		};

		for member in members {
			let member_key = SetMemberKey::new(key.clone(), member);
			let encoded_key = member_key.encode();
			let exists = self
				.get_with_meta(&self.set_db, encoded_key.clone())
				.await?
				.is_some_and(|entry| entry.seq >= meta_val.version);

			if exists {
				self.set_db
					.delete_with_options(encoded_key, &write_opts)
					.await?;
				removed_count += 1;
			}
		}

		if removed_count > 0 {
			meta_val.len -= removed_count;
			if meta_val.len == 0 {
				self.string_db
					.delete_with_options(meta_encoded_key, &write_opts)
					.await?;
			} else {
				let put_opts = PutOptions::default();
				self.string_db
					.put_with_options(meta_encoded_key, meta_val.encode(), &put_opts, &write_opts)
					.await?;
			}
		}

		Ok(removed_count)
	}

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
}
