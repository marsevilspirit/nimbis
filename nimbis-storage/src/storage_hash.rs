use std::collections::BTreeMap;
use std::collections::BTreeSet;

use bytes::Bytes;
use futures::future;
use nimbis_macros::storage_lock;
use slatedb::WriteBatch;
use slatedb::config::PutOptions;

use crate::data_type::DataType;
use crate::error::StorageError;
use crate::hash::field_key::HashFieldKey;
use crate::storage::Storage;
use crate::string::meta::HashMetaValue;
use crate::top_level_key::TopLevelKey;
use crate::typed_db::MetadataChange;

impl Storage {
	#[fastrace::trace]
	pub async fn hset(&self, key: Bytes, field: Bytes, value: Bytes) -> Result<i64, StorageError> {
		self.hset_many(key, vec![(field, value)]).await
	}

	#[storage_lock(write, key, DataType::Hash)]
	#[fastrace::trace]
	pub async fn hset_many(
		&self,
		key: Bytes,
		fields: Vec<(Bytes, Bytes)>,
	) -> Result<i64, StorageError> {
		let fields: BTreeMap<_, _> = fields.into_iter().collect();
		if fields.is_empty() {
			return Ok(0);
		}

		let put_opts = PutOptions::default();
		let meta_val = self.hash_db.load(&key).await?;
		let mut batch = WriteBatch::new();

		let Some(mut meta_val) = meta_val else {
			for (field, value) in &fields {
				let field_key = HashFieldKey::new(key.clone(), field.clone());
				batch.put_with_options(field_key.encode()?, value, &put_opts);
			}
			// SlateDB assigns one commit sequence to every row in a WriteBatch. Zero
			// therefore means "use this metadata row's sequence" on read.
			let new_meta = HashMetaValue::new(0, fields.len() as u64);
			self.hash_db
				.commit(&key, batch, MetadataChange::Put(new_meta))
				.await?;
			return Ok(fields.len() as i64);
		};

		let mut added_count = 0u64;
		for (field, value) in fields {
			let field_key = HashFieldKey::new(key.clone(), field);
			let encoded_field_key = field_key.encode()?;
			let exists = self
				.hash_db
				.get_entry(encoded_field_key.clone())
				.await?
				.is_some_and(|kv| kv.seq >= meta_val.version);
			if !exists {
				added_count += 1;
			}
			batch.put_with_options(encoded_field_key, value, &put_opts);
		}

		if added_count > 0 {
			meta_val.len = meta_val.len.checked_add(added_count).ok_or_else(|| {
				StorageError::DataInconsistency {
					message: "hash metadata length overflow".to_string(),
				}
			})?;
		}

		self.hash_db
			.commit(&key, batch, MetadataChange::Put(meta_val))
			.await?;
		Ok(added_count as i64)
	}

	#[storage_lock(read, key, DataType::Hash)]
	#[fastrace::trace]
	pub async fn hget(&self, key: Bytes, field: Bytes) -> Result<Option<Bytes>, StorageError> {
		// Check if the hash exists and is valid, get version
		let Some(meta_val) = self.hash_db.load(&key).await? else {
			return Ok(None);
		};

		let field_key = HashFieldKey::new(key, field);
		let result = self.hash_db.get_entry(field_key.encode()?).await?;
		if let Some(kv) = result
			&& kv.seq >= meta_val.version
		{
			return Ok(Some(kv.value));
		}
		Ok(None)
	}

	#[storage_lock(read, key, DataType::Hash)]
	#[fastrace::trace]
	pub async fn hlen(&self, key: Bytes) -> Result<u64, StorageError> {
		if let Some(meta_val) = self.hash_db.load(&key).await? {
			Ok(meta_val.len)
		} else {
			Ok(0)
		}
	}

	#[storage_lock(read, key, DataType::Hash)]
	#[fastrace::trace]
	pub async fn hmget(
		&self,
		key: Bytes,
		fields: &[Bytes],
	) -> Result<Vec<Option<Bytes>>, StorageError> {
		// Check if the hash exists and is valid, get version
		let Some(meta_val) = self.hash_db.load(&key).await? else {
			return Ok(vec![None; fields.len()]);
		};
		let version = meta_val.version;

		// Create a future for each field lookup to enable concurrent execution
		// These futures will be awaited in parallel using try_join_all below
		let futures: Vec<_> = fields
			.iter()
			.map(|field| {
				// We don't need to call self.hget() which repeats the check, we can access
				// hash_db directly
				let field_key = HashFieldKey::new(key.clone(), field.clone());
				// We need to clone the db handle for the closure/future if needed, but
				// self.hash_db is Arc Actually self.hash_db.get is async.
				// We can just call self.hash_db.get
				async move {
					let k = field_key.encode()?;
					self.hash_db.get_entry(k).await
				}
			})
			.collect();

		// The error handling types need to match. hash_db.get returns SlateDB error.
		// hget returns Box<dyn Error>.
		// try_join_all expects futures to return Result<T, E> where E is same.
		// slateDB errors satisfy Into<Box<dyn Error>>? Maybe.
		// Let's keep it simple and use a loop or just map errors.
		// Or verify if try_join_all works with SlateDB errors directly.
		// For simplicity/safety, let's just map the results.

		let results = future::try_join_all(futures).await?;
		Ok(results
			.into_iter()
			.map(|kv| match kv {
				Some(kv) if kv.seq >= version => Some(kv.value),
				_ => None,
			})
			.collect())
	}

	#[storage_lock(read, key, DataType::Hash)]
	#[fastrace::trace]
	pub async fn hgetall(&self, key: Bytes) -> Result<Vec<(Bytes, Bytes)>, StorageError> {
		// Check if the hash exists and is valid, get version
		let Some(meta_val) = self.hash_db.load(&key).await? else {
			return Ok(Vec::new());
		};

		// Construct prefix: len(user_key) + user_key
		let top_level_key = TopLevelKey::new(key.clone())?;
		let prefix = top_level_key.encode();

		// Keep the range bounded to this hash and exclude its exact metadata key.
		// Once metadata and fields share the same DB, an unbounded scan prepares
		// iterators for unrelated hot ranges, while starting at the metadata key
		// also merges its potentially deep update history unnecessarily.
		let mut stream = self
			.hash_db
			.scan_entries(top_level_key.sub_key_range()?)
			.await?;
		let mut results = Vec::new();

		while let Some(kv) = stream.next().await? {
			let k = kv.key;
			let v = kv.value;
			if !k.starts_with(&prefix) {
				break;
			}

			if kv.seq < meta_val.version {
				continue;
			}

			let Ok(field_key) = HashFieldKey::decode(&k) else {
				continue;
			};
			results.push((field_key.field().clone(), v));
		}

		Ok(results)
	}

	#[storage_lock(write, key, DataType::Hash)]
	#[fastrace::trace]
	pub async fn hdel(&self, key: Bytes, fields: &[Bytes]) -> Result<i64, StorageError> {
		let mut meta_val = match self.hash_db.load(&key).await? {
			Some(meta) => meta,
			None => return Ok(0),
		};
		let fields: BTreeSet<_> = fields.iter().cloned().collect();
		let mut encoded_fields = Vec::new();

		for field in fields {
			let field_key = HashFieldKey::new(key.clone(), field);
			let encoded_field_key = field_key.encode()?;
			let exists = self
				.hash_db
				.get_entry(encoded_field_key.clone())
				.await?
				.is_some_and(|kv| kv.seq >= meta_val.version);
			if exists {
				encoded_fields.push(encoded_field_key);
			}
		}

		if encoded_fields.is_empty() {
			return Ok(0);
		}

		let deleted_count = encoded_fields.len() as u64;
		if deleted_count > meta_val.len {
			return Err(StorageError::DataInconsistency {
				message: "hash metadata length is smaller than deleted field count".to_string(),
			});
		}

		let mut batch = WriteBatch::new();
		for encoded_field in encoded_fields {
			batch.delete(encoded_field);
		}
		meta_val.len -= deleted_count;
		let metadata = if meta_val.len == 0 {
			MetadataChange::Delete
		} else {
			MetadataChange::Put(meta_val)
		};
		self.hash_db.commit(&key, batch, metadata).await?;

		Ok(deleted_count as i64)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::data_type::DataType;
	use crate::top_level_key::MAX_ENCODED_KEY_LEN;
	use crate::top_level_key::MAX_USER_KEY_LEN;

	fn metric<V>(db: &crate::typed_db::TypedDb<V>, name: &'static str) -> i64 {
		db.metric(name)
	}

	async fn get_storage() -> (Storage, std::path::PathBuf) {
		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_test_hash_{}", timestamp));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		(storage, path)
	}

	#[tokio::test]
	async fn hset_rejects_a_composite_key_that_exceeds_the_codec_limit() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from(vec![b'k'; MAX_USER_KEY_LEN]);

		let error = storage
			.hset(key.clone(), Bytes::from_static(b"field"), Bytes::new())
			.await
			.unwrap_err();
		assert!(matches!(
			error,
			StorageError::InvalidKeyLength {
				length,
				max: MAX_ENCODED_KEY_LEN
			} if length > MAX_ENCODED_KEY_LEN
		));
		assert!(
			storage
				.hash_db
				.raw()
				.get(TopLevelKey::new(key).unwrap().encode())
				.await
				.unwrap()
				.is_none()
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_hset_hget() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("myhash");
		let field = Bytes::from("f1");
		let val = Bytes::from("v1");

		// HSET returns 1 for new field
		let res = storage
			.hset(key.clone(), field.clone(), val.clone())
			.await
			.unwrap();
		assert_eq!(res, 1);

		// HGET returns value
		let got = storage.hget(key.clone(), field.clone()).await.unwrap();
		assert_eq!(got, Some(val.clone()));

		// HLEN returns 1
		let len = storage.hlen(key.clone()).await.unwrap();
		assert_eq!(len, 1);

		// HSET overwrite
		let val2 = Bytes::from("v2");
		let res = storage
			.hset(key.clone(), field.clone(), val2.clone())
			.await
			.unwrap();
		assert_eq!(res, 0); // 0 for update

		// HGET returns new value
		let got = storage.hget(key.clone(), field.clone()).await.unwrap();
		assert_eq!(got, Some(val2.clone()));

		// HLEN still 1
		let len = storage.hlen(key.clone()).await.unwrap();
		assert_eq!(len, 1);

		// HSET another field
		let field2 = Bytes::from("f2");
		let val2_initial = Bytes::from("v2_initial");
		storage
			.hset(key.clone(), field2.clone(), val2_initial.clone())
			.await
			.unwrap();

		// HMGET
		let results = storage
			.hmget(
				key.clone(),
				&[field.clone(), field2.clone(), Bytes::from("missing")],
			)
			.await
			.unwrap();
		assert_eq!(results.len(), 3);
		assert_eq!(results[0], Some(val2.clone()));
		assert_eq!(results[1], Some(val2_initial.clone()));
		assert_eq!(results[2], None);

		// HGETALL
		let all = storage.hgetall(key.clone()).await.unwrap();
		// Since iterator order might be lexicographical by key (user_key+len+field)
		// keys: "myhash" + ... "f1" ...
		// keys: "myhash" + ... "f2" ...
		// f1 < f2.
		assert_eq!(all.len(), 2);
		// We can sort to be sure or check contains.
		let mut sorted = all;
		sorted.sort_by(|a, b| a.0.cmp(&b.0));
		assert_eq!(sorted[0], (field.clone(), val2));
		assert_eq!(sorted[1], (field2.clone(), val2_initial));

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_hash_metadata_is_colocated_and_pending_version_is_resolved() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("colocated_hash_meta");
		let meta_key = TopLevelKey::new(key.clone()).unwrap().encode();

		let added = storage
			.hset(key.clone(), Bytes::from("field"), Bytes::from("value"))
			.await
			.unwrap();
		assert_eq!(added, 1);

		let raw_meta = storage
			.hash_db
			.raw()
			.get_key_value(meta_key.clone())
			.await
			.unwrap()
			.expect("hash metadata should be stored in hash_db");
		let encoded_meta = HashMetaValue::decode(&raw_meta.value).unwrap();
		assert_eq!(encoded_meta.version, 0, "zero marks a pending version");
		assert!(raw_meta.seq > 0);
		let raw_field = storage
			.hash_db
			.raw()
			.get_key_value(
				HashFieldKey::new(key.clone(), Bytes::from("field"))
					.encode()
					.unwrap(),
			)
			.await
			.unwrap()
			.expect("hash field should be committed with its metadata");
		assert_eq!(raw_field.seq, raw_meta.seq);

		let resolved_meta = storage
			.hash_db
			.load(&key)
			.await
			.unwrap()
			.expect("hash metadata should resolve from hash_db");
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
	async fn test_hash_ttl_rewrite_preserves_generation_after_reopen() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("hash_ttl_generation");
		let meta_key = TopLevelKey::new(key.clone()).unwrap().encode();
		storage
			.hset(key.clone(), Bytes::from("f1"), Bytes::from("v1"))
			.await
			.unwrap();

		let initial_meta = storage
			.hash_db
			.raw()
			.get_key_value(meta_key.clone())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(
			HashMetaValue::decode(&initial_meta.value).unwrap().version,
			0
		);
		let generation = initial_meta.seq;
		let expire_time =
			(chrono::Utc::now().timestamp_millis().max(0) as u64).saturating_add(60_000);
		assert!(
			storage
				.expire(DataType::Hash, key.clone(), expire_time)
				.await
				.unwrap()
		);

		let rewritten_meta = storage
			.hash_db
			.raw()
			.get_key_value(meta_key)
			.await
			.unwrap()
			.unwrap();
		let rewritten_meta = HashMetaValue::decode(&rewritten_meta.value).unwrap();
		assert_eq!(rewritten_meta.version, generation);
		assert_eq!(rewritten_meta.expire_time, expire_time);

		storage
			.hset(key.clone(), Bytes::from("f2"), Bytes::from("v2"))
			.await
			.unwrap();
		storage.close().await.unwrap();
		drop(storage);

		let storage = Storage::open(&path, None).await.unwrap();
		assert_eq!(storage.hlen(key.clone()).await.unwrap(), 2);
		assert_eq!(
			storage.hget(key.clone(), Bytes::from("f1")).await.unwrap(),
			Some(Bytes::from("v1"))
		);
		assert_eq!(
			storage.hget(key.clone(), Bytes::from("f2")).await.unwrap(),
			Some(Bytes::from("v2"))
		);
		assert!(storage.ttl(DataType::Hash, key).await.unwrap().unwrap() > 0);
		storage.close().await.unwrap();
		drop(storage);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_hset_many_duplicate_fields_use_last_value_and_count_once() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("hset_many_duplicates");
		let f1 = Bytes::from("f1");
		let f2 = Bytes::from("f2");
		let f3 = Bytes::from("f3");

		let added = storage
			.hset_many(
				key.clone(),
				vec![
					(f1.clone(), Bytes::from("first")),
					(f2.clone(), Bytes::from("second")),
					(f1.clone(), Bytes::from("last")),
				],
			)
			.await
			.unwrap();
		assert_eq!(added, 2);
		assert_eq!(storage.hlen(key.clone()).await.unwrap(), 2);
		assert_eq!(
			storage.hget(key.clone(), f1.clone()).await.unwrap(),
			Some(Bytes::from("last"))
		);

		let added = storage
			.hset_many(
				key.clone(),
				vec![
					(f1.clone(), Bytes::from("updated")),
					(f3.clone(), Bytes::from("before_last")),
					(f3.clone(), Bytes::from("new_last")),
				],
			)
			.await
			.unwrap();
		assert_eq!(added, 1);
		assert_eq!(storage.hlen(key.clone()).await.unwrap(), 3);
		assert_eq!(
			storage.hget(key.clone(), f1).await.unwrap(),
			Some(Bytes::from("updated"))
		);
		assert_eq!(
			storage.hget(key, f3).await.unwrap(),
			Some(Bytes::from("new_last"))
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_hset_many_rejects_metadata_length_overflow_without_writing_batch() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("hset_length_overflow");
		let existing_field = Bytes::from("existing");
		let new_field = Bytes::from("new");
		storage
			.hset(key.clone(), existing_field.clone(), Bytes::from("original"))
			.await
			.unwrap();

		let mut meta = storage.hash_db.load(&key).await.unwrap().unwrap();
		meta.len = u64::MAX;
		let meta_key = TopLevelKey::new(key.clone()).unwrap().encode();
		storage
			.hash_db
			.raw()
			.put(meta_key.clone(), meta.encode())
			.await
			.unwrap();

		let error = storage
			.hset(key.clone(), new_field.clone(), Bytes::from("value"))
			.await
			.unwrap_err();
		assert!(matches!(
			error,
			StorageError::DataInconsistency { message }
				if message == "hash metadata length overflow"
		));

		assert!(
			storage
				.hash_db
				.raw()
				.get(HashFieldKey::new(key.clone(), new_field).encode().unwrap(),)
				.await
				.unwrap()
				.is_none()
		);
		assert_eq!(
			storage.hget(key.clone(), existing_field).await.unwrap(),
			Some(Bytes::from("original"))
		);
		let stored_meta = storage.hash_db.raw().get(meta_key).await.unwrap().unwrap();
		assert_eq!(HashMetaValue::decode(&stored_meta).unwrap().len, u64::MAX);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_hset_many_uses_one_hash_write_batch() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("single_hash_batch");
		let before_batches = metric(&storage.hash_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.hash_db, "slatedb.db.write_ops");

		let added = storage
			.hset_many(
				key.clone(),
				vec![
					(Bytes::from("f1"), Bytes::from("v1")),
					(Bytes::from("f2"), Bytes::from("v2")),
				],
			)
			.await
			.unwrap();
		assert_eq!(added, 2);

		assert_eq!(
			metric(&storage.hash_db, "slatedb.db.write_batch_count") - before_batches,
			1
		);
		assert_eq!(
			metric(&storage.hash_db, "slatedb.db.write_ops") - before_ops,
			3
		);

		let before_batches = metric(&storage.hash_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.hash_db, "slatedb.db.write_ops");
		assert_eq!(
			storage
				.hset(key.clone(), Bytes::from("f1"), Bytes::from("v3"))
				.await
				.unwrap(),
			0
		);
		assert_eq!(
			metric(&storage.hash_db, "slatedb.db.write_batch_count") - before_batches,
			1
		);
		assert_eq!(
			metric(&storage.hash_db, "slatedb.db.write_ops") - before_ops,
			2
		);

		let meta = storage
			.hash_db
			.raw()
			.get_key_value(TopLevelKey::new(key.clone()).unwrap().encode())
			.await
			.unwrap()
			.unwrap();
		let field = storage
			.hash_db
			.raw()
			.get_key_value(HashFieldKey::new(key, Bytes::from("f1")).encode().unwrap())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(meta.seq, field.seq);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_hdel() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("myhash_del");
		let f1 = Bytes::from("f1");
		let f2 = Bytes::from("f2");
		let v1 = Bytes::from("v1");
		let v2 = Bytes::from("v2");

		// Setup
		storage
			.hset(key.clone(), f1.clone(), v1.clone())
			.await
			.unwrap();
		storage
			.hset(key.clone(), f2.clone(), v2.clone())
			.await
			.unwrap();

		// HDEL one field
		let count = storage
			.hdel(key.clone(), std::slice::from_ref(&f1))
			.await
			.unwrap();
		assert_eq!(count, 1);

		// Verify f1 gone, f2 remains
		let val1 = storage.hget(key.clone(), f1.clone()).await.unwrap();
		assert_eq!(val1, None);
		let val2 = storage.hget(key.clone(), f2.clone()).await.unwrap();
		assert_eq!(val2, Some(v2.clone()));
		let len = storage.hlen(key.clone()).await.unwrap();
		assert_eq!(len, 1);

		// HDEL missing field
		let count = storage
			.hdel(key.clone(), &[Bytes::from("missing")])
			.await
			.unwrap();
		assert_eq!(count, 0);

		// HDEL remaining field (should delete hash meta)
		let count = storage
			.hdel(key.clone(), std::slice::from_ref(&f2))
			.await
			.unwrap();
		assert_eq!(count, 1);

		// Verify empty
		let len = storage.hlen(key.clone()).await.unwrap();
		assert_eq!(len, 0);

		let exists = storage.exists(DataType::Hash, key.clone()).await.unwrap();
		assert!(!exists);

		// Cleanup
		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_hdel_duplicate_fields_count_once() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("hdel_duplicate_fields");
		let f1 = Bytes::from("f1");
		let f2 = Bytes::from("f2");
		storage
			.hset_many(
				key.clone(),
				vec![
					(f1.clone(), Bytes::from("v1")),
					(f2.clone(), Bytes::from("v2")),
				],
			)
			.await
			.unwrap();

		let deleted = storage
			.hdel(key.clone(), &[f1.clone(), f1.clone()])
			.await
			.unwrap();
		assert_eq!(deleted, 1);
		assert_eq!(storage.hlen(key.clone()).await.unwrap(), 1);
		assert_eq!(storage.hget(key.clone(), f1).await.unwrap(), None);
		assert_eq!(
			storage.hget(key.clone(), f2.clone()).await.unwrap(),
			Some(Bytes::from("v2"))
		);

		let deleted = storage.hdel(key.clone(), &[f2.clone(), f2]).await.unwrap();
		assert_eq!(deleted, 1);
		assert_eq!(storage.hlen(key).await.unwrap(), 0);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_hset_recreate_same_field_after_del() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("myhash_recreate_same_field");
		let field = Bytes::from("f1");

		let created = storage
			.hset(key.clone(), field.clone(), Bytes::from("v1"))
			.await
			.unwrap();
		assert_eq!(created, 1);

		let deleted = storage.del(DataType::Hash, [key.clone()]).await.unwrap();
		assert_eq!(deleted, 1);

		let recreated = storage
			.hset(key.clone(), field.clone(), Bytes::from("v2"))
			.await
			.unwrap();
		assert_eq!(recreated, 1);

		let len = storage.hlen(key.clone()).await.unwrap();
		assert_eq!(len, 1);

		let got = storage.hget(key.clone(), field.clone()).await.unwrap();
		assert_eq!(got, Some(Bytes::from("v2")));

		let _ = std::fs::remove_dir_all(path);
	}
}
