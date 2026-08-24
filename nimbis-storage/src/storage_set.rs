use std::collections::HashSet;

use bytes::Bytes;
use nimbis_macros::storage_lock;
use slatedb::WriteBatch;
use slatedb::config::PutOptions;

use crate::data_type::DataType;
use crate::error::StorageError;
use crate::set::member_key::SetMemberKey;
use crate::storage::Storage;
use crate::string::meta::SetMetaValue;
use crate::top_level_key::TopLevelKey;
use crate::typed_db::MetadataChange;

impl Storage {
	#[storage_lock(write, key, DataType::Set)]
	#[fastrace::trace]
	pub async fn sadd(&self, key: Bytes, members: Vec<Bytes>) -> Result<u64, StorageError> {
		let put_opts = PutOptions::default();

		let (mut meta_val, meta_missing) = match self.set_db.load(&key).await? {
			Some(meta) => (meta, false),
			None => (SetMetaValue::new(0, 0), true),
		};

		// Deduplicate members, keeping the first occurrence
		let mut unique_members = HashSet::new();
		let members: Vec<_> = members
			.into_iter()
			.filter(|m| unique_members.insert(m.clone()))
			.collect();
		if members.is_empty() {
			return Ok(0);
		}

		let mut added_member_keys = Vec::new();

		for member in members {
			let member_key = SetMemberKey::new(key.clone(), member);
			let encoded_member_key = member_key.encode()?;
			let exists = if meta_missing {
				false
			} else {
				self.set_db
					.get_entry(encoded_member_key.clone())
					.await?
					.is_some_and(|kv| kv.seq >= meta_val.version)
			};

			if !exists {
				added_member_keys.push(encoded_member_key);
			}
		}

		let added_count = added_member_keys.len() as u64;
		if added_count == 0 {
			return Ok(0);
		}

		meta_val.len = meta_val.len.checked_add(added_count).ok_or_else(|| {
			StorageError::DataInconsistency {
				message: "set metadata length overflow after SADD".to_string(),
			}
		})?;
		let mut batch = WriteBatch::new();
		for member_key in added_member_keys {
			batch.put_with_options(
				member_key,
				Bytes::new(), // value is empty for set members
				&put_opts,
			);
		}
		// A fresh collection keeps version=0 on disk. All rows in this batch receive
		// the same sequence, and TypedDb::load resolves zero to that sequence.
		self.set_db
			.commit(&key, batch, MetadataChange::Put(meta_val))
			.await?;

		Ok(added_count)
	}

	#[storage_lock(read, key, DataType::Set)]
	#[fastrace::trace]
	pub async fn smembers(&self, key: Bytes) -> Result<Vec<Bytes>, StorageError> {
		let Some(meta_val) = self.set_db.load(&key).await? else {
			return Ok(Vec::new());
		};

		// Construct prefix: len(user_key) + user_key
		let top_level_key = TopLevelKey::new(key.clone())?;
		let prefix = top_level_key.encode();

		let mut stream = self
			.set_db
			.scan_entries(top_level_key.sub_key_range()?)
			.await?;
		let mut members = Vec::new();

		while let Some(kv) = stream.next().await? {
			let k = kv.key;
			if !k.starts_with(&prefix) {
				break;
			}
			if kv.seq < meta_val.version {
				continue;
			}

			let Ok(member_key) = SetMemberKey::decode(&k) else {
				continue;
			};
			members.push(member_key.member().clone());
		}

		Ok(members)
	}

	#[storage_lock(read, key, DataType::Set)]
	#[fastrace::trace]
	pub async fn sismember(&self, key: Bytes, member: Bytes) -> Result<bool, StorageError> {
		let Some(meta_val) = self.set_db.load(&key).await? else {
			return Ok(false);
		};

		let member_key = SetMemberKey::new(key, member);
		let found = self
			.set_db
			.get_entry(member_key.encode()?)
			.await?
			.is_some_and(|kv| kv.seq >= meta_val.version);
		Ok(found)
	}

	#[storage_lock(write, key, DataType::Set)]
	#[fastrace::trace]
	pub async fn srem(&self, key: Bytes, members: Vec<Bytes>) -> Result<u64, StorageError> {
		let mut meta_val = match self.set_db.load(&key).await? {
			Some(val) => val,
			None => return Ok(0),
		};

		let mut unique_members = HashSet::new();
		let members: Vec<_> = members
			.into_iter()
			.filter(|member| unique_members.insert(member.clone()))
			.collect();
		if members.is_empty() {
			return Ok(0);
		}

		let mut removed_member_keys = Vec::new();
		for member in members {
			let member_key = SetMemberKey::new(key.clone(), member);
			let encoded_key = member_key.encode()?;
			let exists = self
				.set_db
				.get_entry(encoded_key.clone())
				.await?
				.is_some_and(|kv| kv.seq >= meta_val.version);

			if exists {
				removed_member_keys.push(encoded_key);
			}
		}

		let removed_count = removed_member_keys.len() as u64;
		if removed_count == 0 {
			return Ok(0);
		}

		meta_val.len = meta_val.len.checked_sub(removed_count).ok_or_else(|| {
			StorageError::DataInconsistency {
				message: "set metadata length is smaller than removed member count".to_string(),
			}
		})?;
		let mut batch = WriteBatch::new();
		for member_key in removed_member_keys {
			batch.delete(member_key);
		}
		let metadata = if meta_val.len == 0 {
			MetadataChange::Delete
		} else {
			MetadataChange::Put(meta_val)
		};
		self.set_db.commit(&key, batch, metadata).await?;

		Ok(removed_count)
	}

	#[storage_lock(read, key, DataType::Set)]
	#[fastrace::trace]
	pub async fn scard(&self, key: Bytes) -> Result<u64, StorageError> {
		if let Some(meta_val) = self.set_db.load(&key).await? {
			Ok(meta_val.len)
		} else {
			Ok(0)
		}
	}
}

#[cfg(test)]
mod tests {
	use slatedb::config::WriteOptions;

	use super::*;
	use crate::data_type::DataType;
	use crate::string::meta::SetMetaValue;
	use crate::typed_db::metadata_put_options;

	fn metric<V>(db: &crate::typed_db::TypedDb<V>, name: &'static str) -> i64 {
		db.metric(name)
	}

	async fn get_storage() -> (Storage, std::path::PathBuf) {
		let timestamp = ulid::Ulid::generate().to_string();
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
	async fn test_set_metadata_is_colocated_and_pending_version_is_resolved() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("colocated_set_meta");
		let m1 = Bytes::from("m1");
		let m2 = Bytes::from("m2");
		let meta_key = TopLevelKey::new(key.clone()).unwrap().encode();

		let added = storage
			.sadd(key.clone(), vec![m1.clone(), m2.clone()])
			.await
			.unwrap();
		assert_eq!(added, 2);

		let raw_meta = storage
			.set_db
			.raw()
			.get_key_value(meta_key.clone())
			.await
			.unwrap()
			.expect("set metadata should be stored in set_db");
		let encoded_meta = SetMetaValue::decode(&raw_meta.value).unwrap();
		assert_eq!(encoded_meta.version, 0, "zero marks a pending version");
		assert_eq!(encoded_meta.len, 2);
		for member in [m1, m2] {
			let raw_member = storage
				.set_db
				.raw()
				.get_key_value(SetMemberKey::new(key.clone(), member).encode().unwrap())
				.await
				.unwrap()
				.expect("set member should be committed with its metadata");
			assert_eq!(raw_member.seq, raw_meta.seq);
		}

		let resolved_meta = storage
			.set_db
			.load(&key)
			.await
			.unwrap()
			.expect("set metadata should resolve from set_db");
		assert_eq!(resolved_meta.version, raw_meta.seq);
		assert_ne!(resolved_meta.version, 0);
		assert!(
			storage
				.string_db
				.raw()
				.get(meta_key)
				.await
				.unwrap()
				.is_none()
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_sadd_uses_one_batch_and_duplicate_only_is_zero_write() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("single_set_batch");
		let m1 = Bytes::from("m1");
		let m2 = Bytes::from("m2");
		let before_batches = metric(&storage.set_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.set_db, "slatedb.db.write_ops");

		let added = storage
			.sadd(key.clone(), vec![m1.clone(), m1.clone(), m2.clone()])
			.await
			.unwrap();
		assert_eq!(added, 2);
		assert_eq!(
			metric(&storage.set_db, "slatedb.db.write_batch_count") - before_batches,
			1
		);
		assert_eq!(
			metric(&storage.set_db, "slatedb.db.write_ops") - before_ops,
			3
		);

		let before_batches = metric(&storage.set_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.set_db, "slatedb.db.write_ops");
		let added = storage
			.sadd(key, vec![m1.clone(), m1, m2.clone(), m2])
			.await
			.unwrap();
		assert_eq!(added, 0);
		assert_eq!(
			metric(&storage.set_db, "slatedb.db.write_batch_count") - before_batches,
			0
		);
		assert_eq!(
			metric(&storage.set_db, "slatedb.db.write_ops") - before_ops,
			0
		);

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
	async fn test_srem_uses_one_batch_and_deduplicates_members() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("single_set_remove_batch");
		let m1 = Bytes::from("m1");
		let m2 = Bytes::from("m2");
		let m3 = Bytes::from("m3");
		storage
			.sadd(key.clone(), vec![m1.clone(), m2.clone(), m3.clone()])
			.await
			.unwrap();

		let before_batches = metric(&storage.set_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.set_db, "slatedb.db.write_ops");
		let removed = storage
			.srem(key.clone(), vec![m1.clone(), m1, m2.clone(), m2])
			.await
			.unwrap();
		assert_eq!(removed, 2);
		assert_eq!(
			metric(&storage.set_db, "slatedb.db.write_batch_count") - before_batches,
			1
		);
		assert_eq!(
			metric(&storage.set_db, "slatedb.db.write_ops") - before_ops,
			3
		);
		assert_eq!(storage.scard(key.clone()).await.unwrap(), 1);

		let before_batches = metric(&storage.set_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.set_db, "slatedb.db.write_ops");
		let removed = storage
			.srem(key.clone(), vec![m3.clone(), m3])
			.await
			.unwrap();
		assert_eq!(removed, 1);
		assert_eq!(
			metric(&storage.set_db, "slatedb.db.write_batch_count") - before_batches,
			1
		);
		assert_eq!(
			metric(&storage.set_db, "slatedb.db.write_ops") - before_ops,
			2
		);
		assert!(
			storage
				.set_db
				.raw()
				.get(TopLevelKey::new(key).unwrap().encode())
				.await
				.unwrap()
				.is_none()
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_set_mutations_preserve_metadata_ttl() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("set_ttl_preserved");
		let m1 = Bytes::from("m1");
		let m2 = Bytes::from("m2");
		let meta_key = TopLevelKey::new(key.clone()).unwrap().encode();
		storage.sadd(key.clone(), vec![m1.clone()]).await.unwrap();

		let mut meta = storage.set_db.load(&key).await.unwrap().unwrap();
		let expire_time =
			(chrono::Utc::now().timestamp_millis().max(0) as u64).saturating_add(60_000);
		meta.expire_time = expire_time;
		let put_opts = metadata_put_options(&meta).unwrap();
		let write_opts = WriteOptions::default();
		storage
			.set_db
			.raw()
			.put_with_options(meta_key.clone(), meta.encode(), &put_opts, &write_opts)
			.await
			.unwrap();

		assert_eq!(
			storage.sadd(key.clone(), vec![m2.clone()]).await.unwrap(),
			1
		);
		let raw_meta = storage
			.set_db
			.raw()
			.get_key_value(meta_key.clone())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(
			SetMetaValue::decode(&raw_meta.value).unwrap().expire_time,
			expire_time
		);
		assert!(raw_meta.expire_ts.is_some());

		assert_eq!(storage.srem(key.clone(), vec![m1]).await.unwrap(), 1);
		let raw_meta = storage
			.set_db
			.raw()
			.get_key_value(meta_key)
			.await
			.unwrap()
			.unwrap();
		assert_eq!(
			SetMetaValue::decode(&raw_meta.value).unwrap().expire_time,
			expire_time
		);
		assert!(raw_meta.expire_ts.is_some());
		assert_eq!(SetMetaValue::decode(&raw_meta.value).unwrap().len, 1);
		assert!(storage.sismember(key, m2).await.unwrap());

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

		let version_v1 = storage.set_db.load(&key).await.unwrap().unwrap().version;

		let added = storage.sadd(key.clone(), vec![m1.clone()]).await.unwrap();
		assert_eq!(added, 0);

		let version_after_dup = storage.set_db.load(&key).await.unwrap().unwrap().version;
		assert_eq!(version_after_dup, version_v1);

		let added = storage.sadd(key.clone(), vec![m2.clone()]).await.unwrap();
		assert_eq!(added, 1);

		let version_after_update = storage.set_db.load(&key).await.unwrap().unwrap().version;
		assert_eq!(version_after_update, version_v1);

		let removed = storage.del(DataType::Set, [key.clone()]).await.unwrap();
		assert_eq!(removed, 1);

		let added = storage.sadd(key.clone(), vec![m1.clone()]).await.unwrap();
		assert_eq!(added, 1);

		let version_v2 = storage.set_db.load(&key).await.unwrap().unwrap().version;
		assert!(version_v2 > version_v1);

		let members = storage.smembers(key.clone()).await.unwrap();
		assert_eq!(members, vec![m1]);

		let _ = std::fs::remove_dir_all(path);
	}
}
