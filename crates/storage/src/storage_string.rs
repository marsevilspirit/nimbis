use bytes::Bytes;

use crate::data_type::DataType;
use crate::expirable::Expirable;
use crate::storage::Storage;
use crate::string::key::StringKey;
use crate::string::meta::HashMetaValue;
use crate::string::meta::ListMetaValue;
use crate::string::meta::SetMetaValue;
use crate::string::meta::ZSetMetaValue;
use crate::string::value::StringValue;

impl Storage {
	/// Helper to clean up collection data when overwriting/deleting
	async fn cleanup_collection_type(
		&self,
		user_key: Bytes,
		data_type: DataType,
		meta: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		match data_type {
			DataType::Hash => self.delete_hash_fields(user_key).await,
			DataType::List => {
				let meta_val = ListMetaValue::decode(meta)?;
				self.delete_list_elements(user_key, &meta_val).await
			}
			DataType::Set => self.delete_set_members(user_key).await,
			DataType::ZSet => self.delete_zset_content(user_key).await,
			_ => Ok(()),
		}
	}

	pub async fn get(
		&self,
		key: Bytes,
	) -> Result<Option<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
		let key_bytes = StringKey::new(key.clone()).encode();
		let db = self.db().clone();
		let result = tokio::task::spawn_blocking(move || db.get(key_bytes)).await??;

		let Some(bytes) = result.filter(|b| !b.is_empty()) else {
			return Ok(None);
		};

		match DataType::from_u8(bytes[0]) {
			Some(DataType::String) => {
				let string_val = StringValue::decode(&bytes)?;
				if string_val.is_expired() {
					self.del(key).await?;
					return Ok(None);
				}
				Ok(Some(string_val.value))
			}
			Some(DataType::Hash | DataType::List | DataType::Set | DataType::ZSet) => {
				Err("WRONGTYPE Operation against a key holding the wrong kind of value".into())
			}
			_ => Err("WRONGTYPE Operation against a key holding the wrong kind of value".into()),
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

		let key_encoded = key.encode();
		let db = self.db().clone();
		let key_encoded_clone = key_encoded.clone();
		let meta_opt = tokio::task::spawn_blocking(move || db.get(key_encoded_clone)).await??;

		// Clean up if it's a collection type
		if let Some(meta) = meta_opt
			&& let Some(dt) = meta.first().and_then(|&b| DataType::from_u8(b))
		{
			self.cleanup_collection_type(user_key, dt, &meta).await?;
		}

		let db = self.db().clone();
		let value_encoded = value.encode();
		tokio::task::spawn_blocking(move || db.put(key_encoded, value_encoded)).await??;
		Ok(())
	}

	pub async fn del(&self, key: Bytes) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
		let user_key = key.clone();
		let key = StringKey::new(key);

		let key_encoded = key.encode();
		let db = self.db().clone();
		let key_encoded_clone = key_encoded.clone();
		let meta = tokio::task::spawn_blocking(move || db.get(key_encoded_clone)).await??;
		let Some(meta) = meta else {
			return Ok(false);
		};

		// Clean up fields if this is a collection type
		if let Some(dt) = meta.first().and_then(|&b| DataType::from_u8(b)) {
			self.cleanup_collection_type(user_key, dt, &meta).await?;
		}

		// Delete from default CF
		let db = self.db().clone();
		tokio::task::spawn_blocking(move || db.delete(key_encoded)).await??;
		Ok(true)
	}

	pub async fn expire(
		&self,
		key: Bytes,
		expire_time: u64,
	) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
		let user_key = key.clone();
		let skey = StringKey::new(key);
		let encoded_key = skey.encode();

		let db = self.db().clone();
		let encoded_key_clone = encoded_key.clone();
		let Some(bytes) = tokio::task::spawn_blocking(move || db.get(encoded_key_clone)).await??
		else {
			return Ok(false);
		};

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
				let mut val = HashMetaValue::decode(&bytes)?;
				val.expire_at(expire_time);
				val.encode()
			}
			Some(DataType::List) => {
				let mut val = ListMetaValue::decode(&bytes)?;
				val.expire_at(expire_time);
				val.encode()
			}
			Some(DataType::Set) => {
				let mut val = SetMetaValue::decode(&bytes)?;
				val.expire_at(expire_time);
				val.encode()
			}
			Some(DataType::ZSet) => {
				let mut val = ZSetMetaValue::decode(&bytes)?;
				val.expire_at(expire_time);
				val.encode()
			}
			_ => return Ok(false),
		};

		// Check if already expired immediately
		let now = chrono::Utc::now().timestamp_millis() as u64;
		if expire_time > 0 && expire_time <= now {
			self.del(user_key).await?;
			return Ok(true);
		}

		let db = self.db().clone();
		tokio::task::spawn_blocking(move || db.put(encoded_key, encoded_val)).await??;
		Ok(true)
	}

	pub async fn ttl(
		&self,
		key: Bytes,
	) -> Result<Option<i64>, Box<dyn std::error::Error + Send + Sync>> {
		let skey = StringKey::new(key);
		let encoded_key = skey.encode();

		let db = self.db().clone();
		let Some(bytes) = tokio::task::spawn_blocking(move || db.get(encoded_key)).await?? else {
			return Ok(None);
		};

		if bytes.is_empty() {
			return Ok(None);
		}

		let remaining_ttl = match DataType::from_u8(bytes[0]) {
			Some(DataType::String) => StringValue::decode(&bytes)?.remaining_ttl(),
			Some(DataType::Hash) => HashMetaValue::decode(&bytes)?.remaining_ttl(),
			Some(DataType::List) => ListMetaValue::decode(&bytes)?.remaining_ttl(),
			Some(DataType::Set) => SetMetaValue::decode(&bytes)?.remaining_ttl(),
			Some(DataType::ZSet) => ZSetMetaValue::decode(&bytes)?.remaining_ttl(),
			_ => return Ok(None),
		};

		match remaining_ttl {
			Some(duration) => Ok(Some(duration.as_millis() as i64)),
			None => Ok(Some(-1)),
		}
	}

	pub async fn exists(
		&self,
		key: Bytes,
	) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
		let user_key = key.clone();
		let skey = StringKey::new(key);
		let encoded_key = skey.encode();

		let db = self.db().clone();
		let Some(bytes) = tokio::task::spawn_blocking(move || db.get(encoded_key)).await?? else {
			return Ok(false);
		};

		if bytes.is_empty() {
			return Ok(false);
		}

		let is_expired = match DataType::from_u8(bytes[0]) {
			Some(DataType::String) => StringValue::decode(&bytes)?.is_expired(),
			Some(DataType::Hash) => HashMetaValue::decode(&bytes)?.is_expired(),
			Some(DataType::List) => ListMetaValue::decode(&bytes)?.is_expired(),
			Some(DataType::Set) => SetMetaValue::decode(&bytes)?.is_expired(),
			Some(DataType::ZSet) => ZSetMetaValue::decode(&bytes)?.is_expired(),
			_ => return Ok(false),
		};

		if is_expired {
			self.del(user_key).await?;
			Ok(false)
		} else {
			Ok(true)
		}
	}

	pub async fn incr(&self, key: Bytes) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
		let current_val = self.get(key.clone()).await?;

		let mut int_val: i64 = match current_val {
			Some(bytes) => {
				// Try to parse string as integer
				let s = std::str::from_utf8(&bytes)?;
				s.parse::<i64>()
					.map_err(|_| "ERR value is not an integer or out of range")?
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
