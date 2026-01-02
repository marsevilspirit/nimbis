use bytes::Bytes;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;

use crate::hash::field_key::HashFieldKey;
use crate::meta::hash_value::HashMetaValue;
use crate::meta::key::MetaKey;
use crate::storage::Storage;

impl Storage {
	pub async fn hset(
		&self,
		key: Bytes,
		field: Bytes,
		value: Bytes,
	) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key.clone());
		let field_key = HashFieldKey::new(key, field);

		// Check if field exists to know if we are updating or adding new
		let existing_field = self.hash_db.get(field_key.encode()).await?;
		let is_new_field = existing_field.is_none();

		// Set the field in hash_db
		let write_opts = WriteOptions {
			await_durable: false,
		};
		self.hash_db
			.put_with_options(
				field_key.encode(),
				value,
				&PutOptions::default(),
				&write_opts,
			)
			.await?;

		// Update metadata in meta_db
		// Optimistic locking / CAS is ideally needed here but for now simplistic approach:
		// Get Meta -> Update -> Put Meta
		// TODO: add lock to avoid race conditions
		let meta_encoded_key = meta_key.encode();
		let current_meta = self.meta_db.get(meta_encoded_key.clone()).await?;

		let mut meta_val = if let Some(meta_bytes) = current_meta {
			HashMetaValue::decode(&meta_bytes)?
		} else {
			HashMetaValue::new(0)
		};

		if is_new_field {
			meta_val.len += 1;
			self.meta_db
				.put_with_options(
					meta_encoded_key,
					meta_val.encode(),
					&PutOptions::default(),
					&write_opts,
				)
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
		let field_key = HashFieldKey::new(key, field);
		let result = self.hash_db.get(field_key.encode()).await?;
		Ok(result)
	}

	pub async fn hlen(&self, key: Bytes) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key);
		let result = self.meta_db.get(meta_key.encode()).await?;
		if let Some(meta_bytes) = result {
			let meta_val = HashMetaValue::decode(&meta_bytes)?;
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
		let mut results = Vec::with_capacity(fields.len());
		for field in fields {
			results.push(self.hget(key.clone(), field.clone()).await?);
		}
		Ok(results)
	}

	pub async fn hgetall(
		&self,
		key: Bytes,
	) -> Result<Vec<(Bytes, Bytes)>, Box<dyn std::error::Error + Send + Sync>> {
		use bytes::Buf;

		let range = key.as_ref()..;
		let mut stream = self.hash_db.scan(range).await?;
		let mut results = Vec::new();

		while let Some(kv) = stream.next().await? {
			let k = kv.key;
			let v = kv.value;

			if !k.starts_with(&key) {
				break;
			}

			// Parse field: user_key + len(u16) + field
			let suffix = &k[key.len()..];
			if suffix.len() < 2 {
				continue;
			}

			let mut buf = suffix;
			let field_len = buf.get_u16() as usize;

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
