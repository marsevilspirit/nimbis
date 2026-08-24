use std::collections::HashSet;

use bytes::Bytes;
use chrono::Utc;
use nimbis_macros::storage_lock;
use slatedb::WriteBatch;
use slatedb::config::PutOptions;
use slatedb::config::Ttl;
use slatedb::config::WriteOptions;

use crate::data_type::DataType;
use crate::error::StorageError;
use crate::storage::Storage;
use crate::string::key::StringKey;
use crate::string::meta::AnyValue;
use crate::string::meta::MetaValue;
use crate::string::value::StringValue;
use crate::utils::is_expired;

impl Storage {
	fn typed_logical_expire_ts(
		data_type: DataType,
		kv: &slatedb::KeyValue,
	) -> Result<Option<i64>, StorageError> {
		if data_type == DataType::String {
			return Ok(kv.expire_ts);
		}

		let value = AnyValue::decode(&kv.value)?;
		if value.data_type() != data_type {
			return Err(StorageError::DataInconsistency {
				message: format!(
					"typed database {data_type:?} contains {:?} top-level metadata",
					value.data_type()
				),
			});
		}
		let expire_time = value.expire_time();
		if expire_time == 0 {
			Ok(kv.expire_ts)
		} else {
			i64::try_from(expire_time)
				.map(Some)
				.map_err(|_| StorageError::DataInconsistency {
					message: "metadata expiration exceeds SlateDB timestamp range".to_string(),
				})
		}
	}

	async fn string_for_update(&self, key: &Bytes) -> Result<Option<Bytes>, StorageError> {
		Ok(Self::get_meta_from_db::<StringValue>(&self.string_db, key)
			.await?
			.map(|value| value.value))
	}

	async fn key_exists(&self, data_type: DataType, key: &Bytes) -> Result<bool, StorageError> {
		let encoded_key = StringKey::new(key.clone()).encode();
		let db = self.db_for_type(data_type);
		let Some(kv) = db.get_key_value(encoded_key.clone()).await? else {
			return Ok(false);
		};
		if !is_expired(Self::typed_logical_expire_ts(data_type, &kv)?) {
			return Ok(true);
		}
		let write_opts = WriteOptions {
			await_durable: false,
		};
		db.delete_with_options(encoded_key, &write_opts).await?;
		Ok(false)
	}

	#[storage_lock(read, key)]
	#[fastrace::trace]
	pub async fn get(&self, key: Bytes) -> Result<Option<Bytes>, StorageError> {
		Ok(Self::get_meta_from_db::<StringValue>(&self.string_db, &key)
			.await?
			.map(|value| value.value))
	}

	#[storage_lock(write, key)]
	#[fastrace::trace]
	pub async fn set(&self, key: Bytes, value: Bytes) -> Result<(), StorageError> {
		let encoded_key = StringKey::new(key).encode();
		let value = StringValue::new(value);

		let write_opts = WriteOptions {
			await_durable: false,
		};
		let put_opts = PutOptions::default();
		self.string_db
			.put_with_options(encoded_key, value.encode(), &put_opts, &write_opts)
			.await?;
		Ok(())
	}

	#[storage_lock(write_many, keys)]
	#[fastrace::trace]
	pub async fn del<I>(&self, data_type: DataType, keys: I) -> Result<i64, StorageError>
	where
		I: IntoIterator<Item = Bytes>,
	{
		let db = self.db_for_type(data_type);
		let mut batch = WriteBatch::new();
		let mut deleted = 0;
		let mut seen = HashSet::new();
		let write_opts = WriteOptions {
			await_durable: false,
		};

		for key in keys {
			let encoded_key = StringKey::new(key).encode();
			if !seen.insert(encoded_key.clone()) {
				continue;
			}
			let Some(kv) = db.get_key_value(encoded_key.clone()).await? else {
				continue;
			};
			if !is_expired(Self::typed_logical_expire_ts(data_type, &kv)?) {
				deleted += 1;
			}
			batch.delete(encoded_key);
		}

		if !batch.is_empty() {
			db.write_with_options(batch, &write_opts).await?;
		}

		Ok(deleted)
	}

	#[storage_lock(write, key)]
	#[fastrace::trace]
	pub async fn expire(
		&self,
		data_type: DataType,
		key: Bytes,
		expire_time: u64,
	) -> Result<bool, StorageError> {
		let encoded_key = StringKey::new(key).encode();
		let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
		let db = self.db_for_type(data_type);
		let write_opts = WriteOptions {
			await_durable: false,
		};
		let Some(kv) = db.get_key_value(encoded_key.clone()).await? else {
			return Ok(false);
		};
		if is_expired(Self::typed_logical_expire_ts(data_type, &kv)?) {
			db.delete_with_options(encoded_key, &write_opts).await?;
			return Ok(false);
		}
		if expire_time > 0 && expire_time <= now {
			db.delete_with_options(encoded_key, &write_opts).await?;
			return Ok(true);
		}

		let mut value = AnyValue::decode(&kv.value)?;
		if value.version() == Some(0) {
			value.set_version(kv.seq);
		}
		value.set_expire_time(expire_time);
		let ttl = if expire_time > 0 {
			let current = chrono::Utc::now().timestamp_millis().max(0) as u64;
			Ttl::ExpireAfter(expire_time.saturating_sub(current))
		} else {
			Ttl::NoExpiry
		};
		let put_opts = PutOptions { ttl };
		db.put_with_options(encoded_key, value.encode(), &put_opts, &write_opts)
			.await?;
		Ok(true)
	}

	#[storage_lock(read, key)]
	#[fastrace::trace]
	pub async fn ttl(&self, data_type: DataType, key: Bytes) -> Result<Option<i64>, StorageError> {
		let encoded_key = StringKey::new(key).encode();
		let db = self.db_for_type(data_type);
		let Some(kv) = db.get_key_value(encoded_key.clone()).await? else {
			return Ok(None);
		};
		let logical_expire_ts = Self::typed_logical_expire_ts(data_type, &kv)?;
		if is_expired(logical_expire_ts) {
			let write_opts = WriteOptions {
				await_durable: false,
			};
			db.delete_with_options(encoded_key, &write_opts).await?;
			return Ok(None);
		}

		Ok(Some(match logical_expire_ts {
			Some(expire_ts) => (expire_ts - Utc::now().timestamp_millis()).max(0),
			None => -1,
		}))
	}

	#[storage_lock(read, key)]
	#[fastrace::trace]
	pub async fn exists(&self, data_type: DataType, key: Bytes) -> Result<bool, StorageError> {
		self.key_exists(data_type, &key).await
	}

	#[storage_lock(read_many, keys)]
	#[fastrace::trace]
	pub async fn exists_many<I>(&self, data_type: DataType, keys: I) -> Result<i64, StorageError>
	where
		I: IntoIterator<Item = Bytes>,
	{
		let mut count = 0;

		for key in keys {
			if self.key_exists(data_type, &key).await? {
				count += 1;
			}
		}

		Ok(count)
	}

	#[storage_lock(write, key)]
	#[fastrace::trace]
	pub async fn incr(&self, key: Bytes) -> Result<i64, StorageError> {
		let current_val = self.string_for_update(&key).await?;

		let mut int_val: i64 = match current_val {
			Some(bytes) => {
				// Try to parse string as integer
				let s = std::str::from_utf8(&bytes)?;
				s.parse::<i64>()
					.map_err(|_| StorageError::DataInconsistency {
						message: "ERR value is not an integer or out of range".to_string(),
					})?
			}
			None => 0,
		};

		int_val = int_val
			.checked_add(1)
			.ok_or_else(|| StorageError::DataInconsistency {
				message: "ERR increment or decrement would overflow".to_string(),
			})?;

		let key = StringKey::new(key);
		let value = StringValue::new(Bytes::from(int_val.to_string()));

		let write_opts = WriteOptions {
			await_durable: false,
		};
		let put_opts = PutOptions::default();
		self.string_db
			.put_with_options(key.encode(), value.encode(), &put_opts, &write_opts)
			.await?;

		Ok(int_val)
	}

	#[storage_lock(write, key)]
	#[fastrace::trace]
	pub async fn decr(&self, key: Bytes) -> Result<i64, StorageError> {
		let current_val = self.string_for_update(&key).await?;

		let mut int_val: i64 = match current_val {
			Some(bytes) => {
				// Try to parse string as integer
				let s = std::str::from_utf8(&bytes)?;
				s.parse::<i64>()
					.map_err(|_| StorageError::DataInconsistency {
						message: "ERR value is not an integer or out of range".to_string(),
					})?
			}
			None => 0,
		};

		int_val = int_val
			.checked_sub(1)
			.ok_or_else(|| StorageError::DataInconsistency {
				message: "ERR increment or decrement would overflow".to_string(),
			})?;

		let key = StringKey::new(key);
		let value = StringValue::new(Bytes::from(int_val.to_string()));

		let write_opts = WriteOptions {
			await_durable: false,
		};
		let put_opts = PutOptions::default();
		self.string_db
			.put_with_options(key.encode(), value.encode(), &put_opts, &write_opts)
			.await?;

		Ok(int_val)
	}

	#[storage_lock(write, key)]
	#[fastrace::trace]
	pub async fn append(&self, key: Bytes, append_val: Bytes) -> Result<usize, StorageError> {
		let current_val = self.string_for_update(&key).await?;

		let new_val = match current_val {
			Some(bytes) => {
				let mut combined = Vec::with_capacity(bytes.len() + append_val.len());
				combined.extend_from_slice(&bytes);
				combined.extend_from_slice(&append_val);
				combined
			}
			None => append_val.to_vec(),
		};

		let len = new_val.len();
		let key = StringKey::new(key);
		let value = StringValue::new(Bytes::from(new_val));

		let write_opts = WriteOptions {
			await_durable: false,
		};
		let put_opts = PutOptions::default();
		self.string_db
			.put_with_options(key.encode(), value.encode(), &put_opts, &write_opts)
			.await?;

		Ok(len)
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	async fn get_storage() -> (Storage, std::path::PathBuf) {
		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_test_{}", timestamp));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		(storage, path)
	}

	const ALL_DATA_TYPES: [DataType; 5] = [
		DataType::String,
		DataType::Hash,
		DataType::List,
		DataType::Set,
		DataType::ZSet,
	];

	fn metric(db: &slatedb::Db, name: &'static str) -> i64 {
		db.metrics()
			.lookup(name)
			.unwrap_or_else(|| panic!("missing SlateDB metric {name}"))
			.get()
	}

	async fn seed_all_types(storage: &Storage, key: &Bytes) {
		storage
			.set(key.clone(), Bytes::from("string"))
			.await
			.unwrap();
		storage
			.hset(key.clone(), Bytes::from("field"), Bytes::from("hash"))
			.await
			.unwrap();
		storage
			.rpush(key.clone(), vec![Bytes::from("list")])
			.await
			.unwrap();
		storage
			.sadd(key.clone(), vec![Bytes::from("set")])
			.await
			.unwrap();
		storage
			.zadd(key.clone(), vec![(1.0, Bytes::from("zset"))])
			.await
			.unwrap();
	}

	#[rstest]
	#[case("key1", "value1")]
	#[case("empty_val", "")]
	#[case("unicode_key_🔑", "unicode_val_🚀")]
	#[case("special_!@#", "value_!@#")]
	#[tokio::test]
	async fn test_storage_string_roundtrip(#[case] key: &str, #[case] value: &str) {
		let (storage, path) = get_storage().await;

		// Test set and get
		storage
			.set(Bytes::from(key.to_string()), Bytes::from(value.to_string()))
			.await
			.unwrap();
		let result = storage.get(Bytes::from(key.to_string())).await.unwrap();
		assert_eq!(result, Some(Bytes::copy_from_slice(value.as_bytes())));

		// Clean up
		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_storage_string_missing() {
		let (storage, path) = get_storage().await;

		let missing = storage.get(Bytes::from("missing")).await.unwrap();
		assert_eq!(missing, None);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_storage_string_overwrite() {
		let (storage, path) = get_storage().await;

		storage
			.set(Bytes::from("key_overwrite"), Bytes::from("val1"))
			.await
			.unwrap();
		let result = storage.get(Bytes::from("key_overwrite")).await.unwrap();
		assert_eq!(result, Some(Bytes::from("val1")));

		storage
			.set(Bytes::from("key_overwrite"), Bytes::from("val2"))
			.await
			.unwrap();
		let result = storage.get(Bytes::from("key_overwrite")).await.unwrap();
		assert_eq!(result, Some(Bytes::from("val2")));

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_typed_del_routes_to_each_database_and_preserves_other_namespaces() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("same-name");
		seed_all_types(&storage, &key).await;

		for data_type in ALL_DATA_TYPES {
			assert!(storage.exists(data_type, key.clone()).await.unwrap());
		}

		for (deleted_index, data_type) in ALL_DATA_TYPES.into_iter().enumerate() {
			assert_eq!(
				storage.del(data_type, [key.clone()]).await.unwrap(),
				1,
				"{data_type:?} should route to its own live namespace"
			);
			for (candidate_index, candidate_type) in ALL_DATA_TYPES.into_iter().enumerate() {
				assert_eq!(
					storage.exists(candidate_type, key.clone()).await.unwrap(),
					candidate_index > deleted_index,
					"deleting {data_type:?} must not change {candidate_type:?}"
				);
			}
		}

		assert_eq!(storage.get(key.clone()).await.unwrap(), None);
		assert_eq!(storage.hlen(key.clone()).await.unwrap(), 0);
		assert_eq!(storage.llen(key.clone()).await.unwrap(), 0);
		assert_eq!(storage.scard(key.clone()).await.unwrap(), 0);
		assert_eq!(storage.zcard(key).await.unwrap(), 0);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_typed_expire_and_ttl_isolate_all_types_and_survive_reopen() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("shared-ttl");
		seed_all_types(&storage, &key).await;
		let expire_time = (Utc::now().timestamp_millis().max(0) as u64).saturating_add(60_000);

		for selected_type in ALL_DATA_TYPES {
			assert!(
				storage
					.expire(selected_type, key.clone(), expire_time)
					.await
					.unwrap()
			);
			for candidate_type in ALL_DATA_TYPES {
				let ttl = storage
					.ttl(candidate_type, key.clone())
					.await
					.unwrap()
					.unwrap();
				if candidate_type == selected_type {
					assert!(ttl > 0, "{selected_type:?} should receive the TTL");
				} else {
					assert_eq!(
						ttl, -1,
						"expiring {selected_type:?} must not change {candidate_type:?}"
					);
				}
			}
			assert!(storage.expire(selected_type, key.clone(), 0).await.unwrap());
			assert_eq!(
				storage.ttl(selected_type, key.clone()).await.unwrap(),
				Some(-1)
			);
		}

		assert!(
			storage
				.expire(DataType::Hash, key.clone(), expire_time)
				.await
				.unwrap()
		);
		let encoded_key = StringKey::new(key.clone()).encode();
		for (data_type, db) in storage.typed_dbs() {
			let kv = db
				.get_key_value(encoded_key.clone())
				.await
				.unwrap()
				.unwrap();
			if data_type == DataType::Hash {
				assert!(kv.expire_ts.is_some());
				assert_eq!(
					AnyValue::decode(&kv.value).unwrap().expire_time(),
					expire_time
				);
			} else {
				assert_eq!(kv.expire_ts, None);
				if data_type != DataType::String {
					assert_eq!(AnyValue::decode(&kv.value).unwrap().expire_time(), 0);
				}
			}
		}
		assert!(
			storage
				.ttl(DataType::Hash, key.clone())
				.await
				.unwrap()
				.unwrap() > 0
		);
		storage.close().await.unwrap();
		drop(storage);

		let storage = Storage::open(&path, None).await.unwrap();
		assert_eq!(
			storage.get(key.clone()).await.unwrap(),
			Some(Bytes::from("string"))
		);
		assert_eq!(
			storage
				.hget(key.clone(), Bytes::from("field"))
				.await
				.unwrap(),
			Some(Bytes::from("hash"))
		);
		assert_eq!(
			storage.lrange(key.clone(), 0, -1).await.unwrap(),
			vec![Bytes::from("list")]
		);
		assert!(
			storage
				.sismember(key.clone(), Bytes::from("set"))
				.await
				.unwrap()
		);
		assert_eq!(
			storage
				.zscore(key.clone(), Bytes::from("zset"))
				.await
				.unwrap(),
			Some(1.0)
		);
		for data_type in ALL_DATA_TYPES {
			let ttl = storage.ttl(data_type, key.clone()).await.unwrap().unwrap();
			if data_type == DataType::Hash {
				assert!(ttl > 0);
			} else {
				assert_eq!(ttl, -1);
			}
		}
		storage.close().await.unwrap();
		drop(storage);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_typed_del_deduplicates_keys_and_uses_one_target_batch() {
		let (storage, path) = get_storage().await;
		let key_a = Bytes::from("typed-del-a");
		let key_b = Bytes::from("typed-del-b");
		let missing = Bytes::from("typed-del-missing");

		for key in [&key_a, &key_b] {
			storage
				.set(key.clone(), Bytes::from("string"))
				.await
				.unwrap();
			storage
				.hset(key.clone(), Bytes::from("field"), Bytes::from("hash"))
				.await
				.unwrap();
		}

		let before_batches = ALL_DATA_TYPES
			.map(|data_type| metric(storage.db_for_type(data_type), "db/write_batch_count"));
		assert_eq!(
			storage
				.del(
					DataType::Hash,
					[
						key_a.clone(),
						key_a.clone(),
						missing.clone(),
						key_b.clone(),
						key_b.clone(),
					],
				)
				.await
				.unwrap(),
			2
		);

		for (index, data_type) in ALL_DATA_TYPES.into_iter().enumerate() {
			let written_batches = metric(storage.db_for_type(data_type), "db/write_batch_count")
				- before_batches[index];
			assert_eq!(
				written_batches,
				if data_type == DataType::Hash { 1 } else { 0 },
				"typed DEL must write only one batch to hash_db"
			);
		}

		assert_eq!(
			storage
				.exists_many(
					DataType::Hash,
					[key_a.clone(), key_b.clone(), missing.clone()]
				)
				.await
				.unwrap(),
			0
		);
		assert_eq!(
			storage
				.exists_many(
					DataType::String,
					[key_a.clone(), key_b.clone(), missing.clone()]
				)
				.await
				.unwrap(),
			2
		);

		let before_missing_delete = metric(&storage.hash_db, "db/write_batch_count");
		assert_eq!(
			storage
				.del(DataType::Hash, [key_a, key_b, missing])
				.await
				.unwrap(),
			0
		);
		assert_eq!(
			metric(&storage.hash_db, "db/write_batch_count"),
			before_missing_delete,
			"an all-missing typed DEL must not submit an empty batch"
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[rstest]
	#[case("counter", None, 1, 2, 1, 0, -1)]
	#[case("negative_start", Some("-5"), -4, -3, -4, -5, -6)]
	#[case(
		"large_start",
		Some("999999999999999"),
		1000000000000000,
		1000000000000001,
		1000000000000000,
		999999999999999,
		999999999999998
	)]
	#[case("zero_start", Some("0"), 1, 2, 1, 0, -1)]
	#[tokio::test]
	async fn test_storage_string_incr_decr(
		#[case] key: &str,
		#[case] initial_val: Option<&str>,
		#[case] inc1: i64,
		#[case] inc2: i64,
		#[case] dec1: i64,
		#[case] dec2: i64,
		#[case] dec3: i64,
	) {
		let (storage, path) = get_storage().await;
		let key_bytes = Bytes::from(key.to_string());

		if let Some(val) = initial_val {
			storage
				.set(key_bytes.clone(), Bytes::from(val.to_string()))
				.await
				.unwrap();
		}

		assert_eq!(storage.incr(key_bytes.clone()).await.unwrap(), inc1);
		assert_eq!(storage.incr(key_bytes.clone()).await.unwrap(), inc2);
		assert_eq!(storage.decr(key_bytes.clone()).await.unwrap(), dec1);
		assert_eq!(storage.decr(key_bytes.clone()).await.unwrap(), dec2);
		assert_eq!(storage.decr(key_bytes.clone()).await.unwrap(), dec3);

		let _ = std::fs::remove_dir_all(path);
	}

	#[rstest]
	#[case("string_key", "not_int")]
	#[case("float_key", "1.5")]
	#[case("large_key", "999999999999999999999999999")]
	#[tokio::test]
	async fn test_storage_string_incr_decr_errors(#[case] key: &str, #[case] val: &str) {
		let (storage, path) = get_storage().await;

		let key_bytes = Bytes::from(key.to_string());
		storage
			.set(key_bytes.clone(), Bytes::from(val.to_string()))
			.await
			.unwrap();

		let err_incr = storage.incr(key_bytes.clone()).await.unwrap_err();
		assert!(
			err_incr
				.to_string()
				.contains("ERR value is not an integer or out of range")
		);

		let err_decr = storage.decr(key_bytes.clone()).await.unwrap_err();
		assert!(
			err_decr
				.to_string()
				.contains("ERR value is not an integer or out of range")
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_storage_string_incr_overflow() {
		let (storage, path) = get_storage().await;

		let key = Bytes::from("max_key");
		storage
			.set(key.clone(), Bytes::from(i64::MAX.to_string()))
			.await
			.unwrap();

		// INCR on i64::MAX should fail instead of panicking or wrapping around
		let res = storage.incr(key).await;
		assert!(res.is_err());
		assert!(
			res.unwrap_err()
				.to_string()
				.contains("ERR increment or decrement would overflow")
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_storage_string_decr_underflow() {
		let (storage, path) = get_storage().await;

		let key = Bytes::from("min_key");
		storage
			.set(key.clone(), Bytes::from(i64::MIN.to_string()))
			.await
			.unwrap();

		// DECR on i64::MIN should fail instead of panicking or wrapping around
		let res = storage.decr(key).await;
		assert!(res.is_err());
		assert!(
			res.unwrap_err()
				.to_string()
				.contains("ERR increment or decrement would overflow")
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[rstest]
	#[case("append_key", None, "Hello", "Hello", 5)]
	#[case("append_key", Some("Hello"), " World", "Hello World", 11)]
	#[case("append_key", Some(""), "Append", "Append", 6)]
	#[tokio::test]
	async fn test_storage_string_append(
		#[case] key: &str,
		#[case] initial_val: Option<&str>,
		#[case] append_val: &str,
		#[case] expected_val: &str,
		#[case] expected_len: usize,
	) {
		let (storage, path) = get_storage().await;
		let key_bytes = Bytes::from(key.to_string());

		if let Some(val) = initial_val {
			storage
				.set(key_bytes.clone(), Bytes::from(val.to_string()))
				.await
				.unwrap();
		}

		let len = storage
			.append(key_bytes.clone(), Bytes::from(append_val.to_string()))
			.await
			.unwrap();
		assert_eq!(len, expected_len);

		let result = storage.get(key_bytes.clone()).await.unwrap();
		assert_eq!(result, Some(Bytes::from(expected_val.to_string())));

		let _ = std::fs::remove_dir_all(path);
	}
}
