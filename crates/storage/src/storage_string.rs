use bytes::Bytes;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;

use crate::data_type::DataType;
use crate::error::StorageError;
use crate::expirable::Expirable;
use crate::storage::Storage;
use crate::string::key::StringKey;
use crate::string::meta::AnyValue;
use crate::string::meta::ListMetaValue;
use crate::string::value::StringValue;

impl Storage {
	pub async fn get(&self, key: Bytes) -> Result<Option<Bytes>, StorageError> {
		match self.get_meta::<AnyValue>(&key).await? {
			Some(AnyValue::String(val)) => Ok(Some(val.value)),
			Some(val) => Err(StorageError::wrong_type(DataType::String, val.data_type())),
			None => Ok(None),
		}
	}

	pub async fn set(&self, key: Bytes, value: Bytes) -> Result<(), StorageError> {
		let user_key = key.clone();
		let key = StringKey::new(key);
		let value = StringValue::new(value);

		let meta_opt = self.string_db.get(key.encode()).await?;

		// Clean up if it's a Hash or List
		if let Some(meta) = meta_opt {
			match meta.first().and_then(|&b| DataType::from_u8(b)) {
				Some(DataType::Hash) => {
					self.delete_hash_fields(user_key).await?;
				}
				Some(DataType::List) => {
					let meta_val = ListMetaValue::decode(&meta)?;
					self.delete_list_elements(user_key, &meta_val).await?;
				}
				Some(DataType::Set) => {
					self.delete_set_members(user_key).await?;
				}
				_ => {}
			}
		}

		let write_opts = WriteOptions {
			await_durable: false,
		};
		let put_opts = PutOptions::default();
		self.string_db
			.put_with_options(key.encode(), value.encode(), &put_opts, &write_opts)
			.await?;
		Ok(())
	}

	pub async fn del(&self, key: Bytes) -> Result<bool, StorageError> {
		let user_key = key.clone();
		let key = StringKey::new(key);

		let Some(meta) = self.string_db.get(key.encode()).await? else {
			return Ok(false);
		};

		// Clean up fields if this is a collection type
		if let Some(dt) = meta.first().and_then(|&b| DataType::from_u8(b)) {
			match dt {
				DataType::Hash => {
					self.delete_hash_fields(user_key).await?;
				}
				DataType::List => {
					let meta_val = ListMetaValue::decode(&meta)?;
					self.delete_list_elements(user_key, &meta_val).await?;
				}
				DataType::Set => {
					self.delete_set_members(user_key).await?;
				}
				_ => {}
			}
		}

		// Delete from string_db
		let write_opts = WriteOptions {
			await_durable: false,
		};
		self.string_db
			.delete_with_options(key.encode(), &write_opts)
			.await?;
		Ok(true)
	}

	pub async fn expire(&self, key: Bytes, expire_time: u64) -> Result<bool, StorageError> {
		let user_key = key.clone();
		let skey = StringKey::new(key);
		let encoded_key = skey.encode();

		if let Some(mut val) = self.get_meta::<AnyValue>(&user_key).await? {
			val.expire_at(expire_time);
			let encoded_val = val.encode();

			// Check if already expired immediately
			let now = chrono::Utc::now().timestamp_millis() as u64;
			if expire_time > 0 && expire_time <= now {
				self.del(user_key).await?;
				return Ok(true);
			}

			let ttl = if expire_time > 0 {
				slatedb::config::Ttl::ExpireAfter(expire_time.saturating_sub(now))
			} else {
				slatedb::config::Ttl::NoExpiry
			};

			let write_opts = WriteOptions {
				await_durable: false,
			};

			let put_opts = PutOptions { ttl };

			self.string_db
				.put_with_options(encoded_key, encoded_val, &put_opts, &write_opts)
				.await?;
			Ok(true)
		} else {
			Ok(false)
		}
	}

	pub async fn ttl(&self, key: Bytes) -> Result<Option<i64>, StorageError> {
		if let Some(val) = self.get_meta::<AnyValue>(&key).await? {
			match val.remaining_ttl() {
				Some(duration) => Ok(Some(duration.as_millis() as i64)),
				None => Ok(Some(-1)),
			}
		} else {
			Ok(None)
		}
	}

	pub async fn exists(&self, key: Bytes) -> Result<bool, StorageError> {
		Ok(self.get_meta::<AnyValue>(&key).await?.is_some())
	}

	pub async fn incr(&self, key: Bytes) -> Result<i64, StorageError> {
		let current_val = self.get(key.clone()).await?;

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

		int_val += 1;

		self.set(key, Bytes::from(int_val.to_string())).await?;

		Ok(int_val)
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	async fn get_storage() -> (Storage, std::path::PathBuf) {
		let timestamp = ulid::Ulid::new().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_test_{}", timestamp));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		(storage, path)
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
	async fn test_collision_string_hash() {
		let (storage, path) = get_storage().await;
		let k = Bytes::from("k");
		let v = Bytes::from("v");
		let f = Bytes::from("f");

		// 1. SET string
		storage.set(k.clone(), v.clone()).await.unwrap();

		// 2. HSET should fail
		let err = storage
			.hset(k.clone(), f.clone(), v.clone())
			.await
			.unwrap_err();
		assert!(
			err.to_string().contains("WRONGTYPE"),
			"Expected WRONGTYPE, got {}",
			err
		);

		// 3. HGET should fail
		let err = storage.hget(k.clone(), f.clone()).await.unwrap_err();
		assert!(err.to_string().contains("WRONGTYPE"));

		// 4. Delete String
		let deleted = storage.del(k.clone()).await.unwrap();
		assert!(deleted);

		// 5. HSET should succeed
		let res = storage.hset(k.clone(), f.clone(), v.clone()).await.unwrap();
		assert_eq!(res, 1);

		// 6. SET should overwrite Hash (and clean up)
		storage.set(k.clone(), Bytes::from("v2")).await.unwrap();

		// 7. Check String is there
		let val = storage.get(k.clone()).await.unwrap();
		assert_eq!(val, Some(Bytes::from("v2")));

		// 8. Check Hash is gone (HGET -> WRONGTYPE, wait, if we deleted hash fields,
		//    HGET logic checks meta first)
		// Since meta is now String, HGET returns WRONGTYPE. Correct.
		let err = storage.hget(k.clone(), f.clone()).await.unwrap_err();
		assert!(err.to_string().contains("WRONGTYPE"));

		let _ = std::fs::remove_dir_all(path);
	}
}
