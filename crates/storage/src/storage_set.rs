use bytes::Buf;
use bytes::Bytes;

use crate::data_type::DataType;
use crate::expirable::Expirable;
use crate::set::member_key::SetMemberKey;
use crate::storage::Storage;
use crate::string::meta::MetaKey;
use crate::string::meta::SetMetaValue;

impl Storage {
	// Helper to get and validate set metadata.
	async fn get_valid_set_meta(
		&self,
		key: &Bytes,
	) -> Result<Option<SetMetaValue>, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key.clone());
		let Some(meta_bytes) = self.db_get(&meta_key.encode()).await? else {
			return Ok(None);
		};

		if meta_bytes.is_empty() {
			return Ok(None);
		}
		if meta_bytes[0] != DataType::Set as u8 {
			return Err("WRONGTYPE Operation against a key holding the wrong kind of value".into());
		}
		let meta_val = SetMetaValue::decode(&meta_bytes)?;
		if meta_val.is_expired() {
			self.del(key.clone()).await?;
			return Ok(None);
		}
		Ok(Some(meta_val))
	}

	pub(crate) async fn delete_set_members(
		&self,
		key: Bytes,
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.delete_with_prefix("set", &Self::create_key_prefix(&key))
			.await
	}

	pub async fn sadd(
		&self,
		key: Bytes,
		members: Vec<Bytes>,
	) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = meta_key.encode();
		let current_meta_bytes = self.db_get(&meta_encoded_key).await?;

		let mut meta_val = if let Some(meta_bytes) = current_meta_bytes {
			if meta_bytes.is_empty() {
				SetMetaValue::new(0)
			} else {
				match DataType::from_u8(meta_bytes[0]) {
					Some(DataType::Set) => SetMetaValue::decode(&meta_bytes)?,
					_ => {
						return Err(
							"WRONGTYPE Operation against a key holding the wrong kind of value"
								.into(),
						);
					}
				}
			}
		} else {
			SetMetaValue::new(0)
		};

		if meta_val.is_expired() {
			self.delete_set_members(key.clone()).await?;
			meta_val = SetMetaValue::new(0);
		}

		let mut added_count = 0;

		for member in members {
			let member_key = SetMemberKey::new(key.clone(), member);
			let encoded_member_key = member_key.encode();

			if self.set_get(&encoded_member_key).await?.is_none() {
				self.set_put(&encoded_member_key, &[]).await?;
				added_count += 1;
			}
		}

		if added_count > 0 {
			meta_val.len += added_count;
			self.db_put(&meta_encoded_key, &meta_val.encode()).await?;
		}

		Ok(added_count)
	}

	pub async fn smembers(
		&self,
		key: Bytes,
	) -> Result<Vec<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
		if self.get_valid_set_meta(&key).await?.is_none() {
			return Ok(Vec::new());
		}

		let prefix = Self::create_key_prefix(&key);
		let results = self.scan_with_prefix("set", &prefix).await?;
		let prefix_len = prefix.len();
		let mut members = Vec::new();

		for (k, _v) in results {
			let k_bytes: Bytes = k.into();
			if !k_bytes.starts_with(&prefix) {
				break;
			}

			let suffix = &k_bytes[prefix_len..];
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

	pub async fn sismember(
		&self,
		key: Bytes,
		member: Bytes,
	) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
		if self.get_valid_set_meta(&key).await?.is_none() {
			return Ok(false);
		}

		let member_key = SetMemberKey::new(key, member);
		Ok(self.set_get(&member_key.encode()).await?.is_some())
	}

	pub async fn srem(
		&self,
		key: Bytes,
		members: Vec<Bytes>,
	) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = meta_key.encode();

		let mut meta_val = match self.get_valid_set_meta(&key).await? {
			Some(val) => val,
			None => return Ok(0),
		};

		let mut removed_count = 0;

		for member in members {
			let member_key = SetMemberKey::new(key.clone(), member);
			let encoded_key = member_key.encode();

			if self.set_get(&encoded_key).await?.is_some() {
				self.set_delete(&encoded_key).await?;
				removed_count += 1;
			}
		}

		if removed_count > 0 {
			meta_val.len -= removed_count;
			if meta_val.len == 0 {
				self.db_delete(&meta_encoded_key).await?;
			} else {
				self.db_put(&meta_encoded_key, &meta_val.encode()).await?;
			}
		}

		Ok(removed_count)
	}

	pub async fn scard(&self, key: Bytes) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		if let Some(meta_val) = self.get_valid_set_meta(&key).await? {
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
		let storage = Storage::open(&path).await.unwrap();
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
