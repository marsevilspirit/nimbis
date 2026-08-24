use bytes::Bytes;
use nimbis_macros::storage_lock;

use crate::data_type::DataType;
use crate::error::StorageError;
use crate::storage::Storage;
use crate::string::value::StringValue;
use crate::top_level_key::TopLevelKey;

impl Storage {
	async fn string_for_update(&self, key: &Bytes) -> Result<Option<Bytes>, StorageError> {
		Ok(self
			.string_db
			.load_value(key)
			.await?
			.map(|value| value.value))
	}

	#[storage_lock(read, key, DataType::String)]
	#[fastrace::trace]
	pub async fn get(&self, key: Bytes) -> Result<Option<Bytes>, StorageError> {
		Ok(self
			.string_db
			.load_value(&key)
			.await?
			.map(|value| value.value))
	}

	#[storage_lock(write, key, DataType::String)]
	#[fastrace::trace]
	pub async fn set(&self, key: Bytes, value: Bytes) -> Result<(), StorageError> {
		let key = TopLevelKey::new(key)?;
		let value = StringValue::new(value);
		self.string_db.store(key, value).await?;
		Ok(())
	}

	#[storage_lock(write, key, DataType::String)]
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

		let key = TopLevelKey::new(key)?;
		let value = StringValue::new(Bytes::from(int_val.to_string()));

		self.string_db.store(key, value).await?;

		Ok(int_val)
	}

	#[storage_lock(write, key, DataType::String)]
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

		let key = TopLevelKey::new(key)?;
		let value = StringValue::new(Bytes::from(int_val.to_string()));

		self.string_db.store(key, value).await?;

		Ok(int_val)
	}

	#[storage_lock(write, key, DataType::String)]
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
		let key = TopLevelKey::new(key)?;
		let value = StringValue::new(Bytes::from(new_val));

		self.string_db.store(key, value).await?;

		Ok(len)
	}
}

#[cfg(test)]
mod tests {
	use chrono::Utc;
	use rstest::rstest;

	use super::*;
	use crate::string::meta::AnyValue;

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

	fn metric<V>(db: &crate::typed_db::TypedDb<V>, name: &'static str) -> i64 {
		db.metric(name)
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
	async fn test_typed_expire_rejects_unrepresentable_deadline_without_mutation() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("invalid-expiration");
		let max_expiration = crate::expiration::MAX_EXPIRATION_TIMESTAMP_MS;
		seed_all_types(&storage, &key).await;
		let encoded_key = TopLevelKey::new(key.clone()).unwrap().encode();

		for data_type in ALL_DATA_TYPES {
			let db = storage.raw_db_for_type(data_type);
			let before = db
				.get_key_value(encoded_key.clone())
				.await
				.unwrap()
				.unwrap();
			let error = storage
				.expire(data_type, key.clone(), u64::MAX)
				.await
				.unwrap_err();
			assert!(matches!(
				error,
				StorageError::InvalidExpiration {
					timestamp: u64::MAX,
					max
				} if max == max_expiration
			));
			let after = db
				.get_key_value(encoded_key.clone())
				.await
				.unwrap()
				.unwrap();
			assert_eq!(after.seq, before.seq);
			assert_eq!(after.value, before.value);
			assert_eq!(after.expire_ts, before.expire_ts);
			assert_eq!(storage.ttl(data_type, key.clone()).await.unwrap(), Some(-1));

			assert!(
				storage
					.expire(data_type, key.clone(), max_expiration)
					.await
					.unwrap()
			);
			assert!(storage.ttl(data_type, key.clone()).await.unwrap().unwrap() > 0);
			assert!(storage.expire(data_type, key.clone(), 0).await.unwrap());
		}

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
		let encoded_key = TopLevelKey::new(key.clone()).unwrap().encode();
		for (data_type, db) in storage.all_raw_dbs() {
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
			.map(|data_type| storage.metric_for_type(data_type, "slatedb.db.write_batch_count"));
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
			let written_batches = storage
				.metric_for_type(data_type, "slatedb.db.write_batch_count")
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

		let before_missing_delete = metric(&storage.hash_db, "slatedb.db.write_batch_count");
		assert_eq!(
			storage
				.del(DataType::Hash, [key_a, key_b, missing])
				.await
				.unwrap(),
			0
		);
		assert_eq!(
			metric(&storage.hash_db, "slatedb.db.write_batch_count"),
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
