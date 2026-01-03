use bytes::Bytes;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;

use crate::data_type::DataType;
use crate::expirable::Expirable;
use crate::storage::Storage;
use crate::string::key::StringKey;
use crate::string::value::StringValue;

impl Storage {
	pub async fn get(
		&self,
		key: Bytes,
	) -> Result<Option<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
		let key = StringKey::new(key);
		let result = self.string_db.get(key.encode()).await?;

		if let Some(bytes) = result {
			if bytes.is_empty() {
				return Ok(None);
			}

			match DataType::from_u8(bytes[0]) {
				Some(DataType::String) => {
					let string_val = StringValue::decode(&bytes)?;
					if string_val.is_expired() {
						self.del(key.encode()).await?;
						return Ok(None);
					}
					Ok(Some(string_val.value))
				}
				_ => {
					Err("WRONGTYPE Operation against a key holding the wrong kind of value".into())
				}
			}
		} else {
			Ok(None)
		}
	}

	pub async fn set(
		&self,
		key: Bytes,
		value: Bytes,
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let user_key = key.clone();
		let key = StringKey::new(key);
		let value = StringValue::new(value);

		if let Some(existing_meta) = self.string_db.get(key.encode()).await? {
			// Clean up if it's a Hash
			if !existing_meta.is_empty() && existing_meta[0] == DataType::Hash as u8 {
				self.delete_hash_fields(user_key).await?;
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

	pub async fn del(&self, key: Bytes) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
		let user_key = key.clone();
		let key = StringKey::new(key);

		if let Some(existing_meta) = self.string_db.get(key.encode()).await? {
			if !existing_meta.is_empty() {
				// Clean up hash fields
				if let Some(DataType::Hash) = DataType::from_u8(existing_meta[0]) {
					self.delete_hash_fields(user_key).await?;
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
		} else {
			Ok(false)
		}
	}

	pub async fn expire(
		&self,
		key: Bytes,
		expire_time: u64,
	) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
		let user_key = key.clone();
		let skey = StringKey::new(key);
		let encoded_key = skey.encode();

		if let Some(bytes) = self.string_db.get(encoded_key.clone()).await? {
			if bytes.is_empty() {
				return Ok(false);
			}

			let encoded_val = match DataType::from_u8(bytes[0]) {
				Some(DataType::String) => {
					let mut val = StringValue::decode(&bytes)?;
					val.expire_at(expire_time);
					val.encode()
				}
				Some(DataType::Hash) => {
					let mut val = crate::string::meta::HashMetaValue::decode(&bytes)?;
					val.expire_at(expire_time);
					val.encode()
				}
				_ => return Ok(false),
			};

			// Check if already expired immediately
			// We can verify by decoding again, but we know the expire_time we just set.
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

	pub async fn ttl(
		&self,
		key: Bytes,
	) -> Result<Option<i64>, Box<dyn std::error::Error + Send + Sync>> {
		let skey = StringKey::new(key);
		let encoded_key = skey.encode();

		if let Some(bytes) = self.string_db.get(encoded_key).await? {
			if bytes.is_empty() {
				return Ok(None);
			}

			let remaining_ttl = match DataType::from_u8(bytes[0]) {
				Some(DataType::String) => StringValue::decode(&bytes)?.remaining_ttl(),
				Some(DataType::Hash) => {
					crate::string::meta::HashMetaValue::decode(&bytes)?.remaining_ttl()
				}
				_ => return Ok(None),
			};

			match remaining_ttl {
				Some(duration) => Ok(Some(duration.as_millis() as i64)),
				None => Ok(Some(-1)),
			}
		} else {
			Ok(None)
		}
	}

	pub async fn exists(
		&self,
		key: Bytes,
	) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
		let user_key = key.clone();
		let skey = StringKey::new(key);
		let encoded_key = skey.encode();

		if let Some(bytes) = self.string_db.get(encoded_key).await? {
			if bytes.is_empty() {
				return Ok(false);
			}

			let is_expired = match DataType::from_u8(bytes[0]) {
				Some(DataType::String) => StringValue::decode(&bytes)?.is_expired(),
				Some(DataType::Hash) => {
					crate::string::meta::HashMetaValue::decode(&bytes)?.is_expired()
				}
				_ => return Ok(false),
			};

			if is_expired {
				self.del(user_key).await?; // Lazy delete
				Ok(false)
			} else {
				Ok(true)
			}
		} else {
			Ok(false)
		}
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
		let storage = Storage::open(&path).await.unwrap();
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

		// 8. Check Hash is gone (HGET -> WRONGTYPE, wait, if we deleted hash fields, HGET logic checks meta first)
		// Since meta is now String, HGET returns WRONGTYPE. Correct.
		let err = storage.hget(k.clone(), f.clone()).await.unwrap_err();
		assert!(err.to_string().contains("WRONGTYPE"));

		let _ = std::fs::remove_dir_all(path);
	}
}
