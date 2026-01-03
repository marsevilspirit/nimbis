use bytes::Buf;
use bytes::Bytes;
use futures::future;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;

use crate::data_type::DataType;
use crate::expirable::Expirable;
use crate::hash::field_key::HashFieldKey;
use crate::storage::Storage;
use crate::string::meta::HashMetaValue;
use crate::string::meta::MetaKey;

impl Storage {
	// Helper to get and validate hash metadata.
	// Returns:
	// - Ok(Some(meta)) if the key is a valid, non-expired Hash
	// - Ok(None) if the key doesn't exist or is expired
	// - Err if the key exists but is of wrong type (e.g., String)
	async fn get_valid_hash_meta(
		&self,
		key: &Bytes,
	) -> Result<Option<HashMetaValue>, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key.clone());
		if let Some(meta_bytes) = self.string_db.get(meta_key.encode()).await? {
			if meta_bytes.is_empty() {
				return Ok(None);
			}
			if meta_bytes[0] != DataType::Hash as u8 {
				return Err(
					"WRONGTYPE Operation against a key holding the wrong kind of value".into(),
				);
			}
			let meta_val = HashMetaValue::decode(&meta_bytes)?;
			if meta_val.is_expired() {
				self.del(key.clone()).await?;
				return Ok(None);
			}
			Ok(Some(meta_val))
		} else {
			Ok(None)
		}
	}

	// Helper to delete all fields of a hash.
	// Used when overwriting a Hash with a String, or deleting a Hash.
	// TODO: This function is temporary; once the compaction filter is implemented,
	// it will be replaced with a custom filter for elegant deletion.
	pub(crate) async fn delete_hash_fields(
		&self,
		key: Bytes,
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let range = key.as_ref()..;
		let mut stream = self.hash_db.scan(range).await?;
		let mut keys_to_delete = Vec::new();

		while let Some(kv) = stream.next().await? {
			let k = kv.key;
			if !k.starts_with(&key) {
				break;
			}
			// Verify suffix
			let suffix = &k[key.len()..];
			if suffix.len() < 4 {
				continue;
			}
			let mut buf = suffix;
			let field_len = buf.get_u32() as usize;
			if buf.len() != field_len {
				continue;
			}

			keys_to_delete.push(k);
		}

		let write_opts = WriteOptions {
			await_durable: false,
		};
		for k in keys_to_delete {
			self.hash_db.delete_with_options(k, &write_opts).await?;
		}
		Ok(())
	}

	pub async fn hset(
		&self,
		key: Bytes,
		field: Bytes,
		value: Bytes,
	) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key.clone());
		let field_key = HashFieldKey::new(key.clone(), field);

		// Valid type check (Must be Hash or None)
		// We read from string_db which holds ALL metadata (StringValue or HashMetaValue)
		let meta_encoded_key = meta_key.encode();
		let current_meta_bytes = self.string_db.get(meta_encoded_key.clone()).await?;

		let mut meta_val = if let Some(meta_bytes) = current_meta_bytes {
			// Check type
			if meta_bytes.is_empty() {
				// Should not happen for valid keys, but treat as new
				HashMetaValue::new(0)
			} else {
				match DataType::from_u8(meta_bytes[0]) {
					Some(DataType::String) => {
						return Err(
							"WRONGTYPE Operation against a key holding the wrong kind of value"
								.into(),
						);
					}
					_ => {
						// It should be a Hash (or valid meta), decode it
						HashMetaValue::decode(&meta_bytes)?
					}
				}
			}
		} else {
			HashMetaValue::new(0)
		};

		// Check expiration in meta_val
		if meta_val.is_expired() {
			// TODO: Expired. Treat as new.
			// We should clean up old hash fields?
			// Yes, if we are overwriting, we should probably delete old fields if we consider the key gone.
			// But `hset` adds to existing.
			// If key is expired, `hset` should start a FRESH hash (empty).
			// So we need to delete old hash fields first.
			self.delete_hash_fields(key.clone()).await?;
			meta_val = HashMetaValue::new(0);
		}

		// Check if field exists in hash_db
		let existing_field = self.hash_db.get(field_key.encode()).await?;
		let is_new_field = existing_field.is_none();

		// Set the field in hash_db
		let write_opts = WriteOptions {
			await_durable: false,
		};
		let put_opts = PutOptions::default();
		self.hash_db
			.put_with_options(field_key.encode(), value, &put_opts, &write_opts)
			.await?;

		// Update metadata in string_db if needed
		if is_new_field {
			meta_val.len += 1;

			let ttl = meta_val
				.remaining_ttl()
				.map(|d| d.as_millis() as u64)
				.map(slatedb::config::Ttl::ExpireAfter)
				.unwrap_or(slatedb::config::Ttl::NoExpiry);

			let put_opts = PutOptions { ttl };

			self.string_db
				.put_with_options(meta_encoded_key, meta_val.encode(), &put_opts, &write_opts)
				.await?;
			Ok(1)
		} else {
			Ok(0)
		}
	}

	pub async fn hget(
		&self,
		key: Bytes,
		field: Bytes,
	) -> Result<Option<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
		// Check if the hash exists and is valid
		if self.get_valid_hash_meta(&key).await?.is_none() {
			return Ok(None);
		}

		let field_key = HashFieldKey::new(key, field);
		let result = self.hash_db.get(field_key.encode()).await?;
		Ok(result)
	}

	pub async fn hlen(&self, key: Bytes) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		if let Some(meta_val) = self.get_valid_hash_meta(&key).await? {
			Ok(meta_val.len)
		} else {
			Ok(0)
		}
	}

	pub async fn hmget(
		&self,
		key: Bytes,
		fields: &[Bytes],
	) -> Result<Vec<Option<Bytes>>, Box<dyn std::error::Error + Send + Sync>> {
		// Check if the hash exists and is valid
		if self.get_valid_hash_meta(&key).await?.is_none() {
			return Ok(vec![None; fields.len()]);
		}

		// Create a future for each field lookup to enable concurrent execution
		// These futures will be awaited in parallel using try_join_all below
		let futures: Vec<_> = fields
			.iter()
			.map(|field| {
				// We don't need to call self.hget() which repeats the check, we can access hash_db directly
				let field_key = HashFieldKey::new(key.clone(), field.clone());
				// We need to clone the db handle for the closure/future if needed, but self.hash_db is Arc
				// Actually self.hash_db.get is async.
				// We can just call self.hash_db.get
				async move {
					let k = field_key.encode();
					self.hash_db.get(k).await
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
		Ok(results)
	}

	pub async fn hgetall(
		&self,
		key: Bytes,
	) -> Result<Vec<(Bytes, Bytes)>, Box<dyn std::error::Error + Send + Sync>> {
		use bytes::Buf;

		// Check if the hash exists and is valid
		if self.get_valid_hash_meta(&key).await?.is_none() {
			return Ok(Vec::new());
		}

		let range = key.as_ref()..;
		let mut stream = self.hash_db.scan(range).await?;
		let mut results = Vec::new();

		while let Some(kv) = stream.next().await? {
			let k = kv.key;
			let v = kv.value;

			if !k.starts_with(&key) {
				break;
			}

			// Parse field: user_key + len(u32) + field
			let suffix = &k[key.len()..];
			if suffix.len() < 4 {
				continue;
			}

			let mut buf = suffix;
			let field_len = buf.get_u32() as usize;

			if buf.len() != field_len {
				continue;
			}

			let field = Bytes::copy_from_slice(buf);
			results.push((field, v));
		}

		Ok(results)
	}
}

#[cfg(test)]
mod tests {

	use super::*;

	async fn get_storage() -> (Storage, std::path::PathBuf) {
		let timestamp = ulid::Ulid::new().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_test_hash_{}", timestamp));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path).await.unwrap();
		(storage, path)
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
}
