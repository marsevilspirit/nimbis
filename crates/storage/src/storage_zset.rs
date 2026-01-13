use bytes::Bytes;
use futures::future;
use rocksdb::WriteBatch;

use crate::data_type::DataType;
use crate::expirable::Expirable;
use crate::storage::Storage;
use crate::string::meta::MetaKey;
use crate::string::meta::ZSetMetaValue;
use crate::zset::member_key::MemberKey;
use crate::zset::score_key::ScoreKey;

impl Storage {
	// Helper to get and validate zset metadata.
	async fn get_valid_zset_meta(
		&self,
		key: &Bytes,
	) -> Result<Option<ZSetMetaValue>, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key.clone());
		let Some(meta_bytes) = self.db_get(&meta_key.encode()).await? else {
			return Ok(None);
		};

		if meta_bytes.is_empty() {
			return Ok(None);
		}
		if meta_bytes[0] != DataType::ZSet as u8 {
			return Err("WRONGTYPE Operation against a key holding the wrong kind of value".into());
		}
		let meta_val = ZSetMetaValue::decode(&meta_bytes)?;
		if meta_val.is_expired() {
			self.delete_zset_content(key.clone()).await?;
			self.db_delete(&meta_key.encode()).await?;
			return Ok(None);
		}
		Ok(Some(meta_val))
	}

	pub(crate) async fn delete_zset_content(
		&self,
		key: Bytes,
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.delete_with_prefix("zset", &Self::create_key_prefix(&key))
			.await
	}

	pub async fn zadd(
		&self,
		key: Bytes,
		elements: Vec<(f64, Bytes)>, // (score, member)
	) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = meta_key.encode();

		// Prepare member fetch futures
		let mut member_encoded_keys = Vec::with_capacity(elements.len());
		for (_, member) in &elements {
			let member_key = MemberKey::new(key.clone(), member.clone());
			member_encoded_keys.push(member_key.encode());
		}

		let mut member_futs = Vec::with_capacity(elements.len());
		for enc in member_encoded_keys.iter() {
			member_futs.push(self.zset_get(enc.to_vec()));
		}

		// Parallel fetch meta and members
		let (meta_res, members_res) = tokio::join!(
			self.db_get(&meta_encoded_key),
			future::join_all(member_futs)
		);

		let current_meta_bytes = meta_res?;

		let mut old_values: Vec<_> = members_res.into_iter().collect::<Result<_, _>>()?;

		let mut meta_val = if let Some(meta_bytes) = current_meta_bytes {
			if meta_bytes.is_empty() {
				ZSetMetaValue::new(0)
			} else {
				match DataType::from_u8(meta_bytes[0]) {
					Some(DataType::ZSet) => ZSetMetaValue::decode(&meta_bytes)?,
					_ => {
						return Err(
							"WRONGTYPE Operation against a key holding the wrong kind of value"
								.into(),
						);
					}
				}
			}
		} else {
			ZSetMetaValue::new(0)
		};

		if meta_val.is_expired() {
			self.delete_zset_content(key.clone()).await?;
			meta_val = ZSetMetaValue::new(0);
			// Key expired, so old members are conceptually gone
			old_values.fill(None);
		}

		let mut added_count = 0;
		let cf_name = "zset".to_string();

		// Use WriteBatch to ensure atomicity of all zset operations
		let mut batch = WriteBatch::default();
		let db = self.db().clone();

		for (idx, (score, member)) in elements.into_iter().enumerate() {
			let encoded_member_key = &member_encoded_keys[idx];
			let old_score_bytes = &old_values[idx];

			if let Some(old_score_bytes) = old_score_bytes {
				// Update existing member
				let old_score =
					ScoreKey::decode_score(u64::from_be_bytes(old_score_bytes[..8].try_into()?));
				if old_score != score {
					// 1. Delete old ScoreKey
					let old_score_key = ScoreKey::new(key.clone(), old_score, member.clone());
					batch.delete_cf(db.cf_handle(&cf_name).unwrap(), old_score_key.encode());

					// 2. Add new ScoreKey
					let new_score_key = ScoreKey::new(key.clone(), score, member.clone());
					batch.put_cf(db.cf_handle(&cf_name).unwrap(), new_score_key.encode(), []);

					// 3. Update MemberKey
					let encoded_score = ScoreKey::encode_score(score);
					batch.put_cf(
						db.cf_handle(&cf_name).unwrap(),
						encoded_member_key.clone(),
						encoded_score.to_be_bytes(),
					);
				}
			} else {
				// New member
				added_count += 1;

				// Add MemberKey
				let encoded_score = ScoreKey::encode_score(score);
				batch.put_cf(
					db.cf_handle(&cf_name).unwrap(),
					encoded_member_key.clone(),
					encoded_score.to_be_bytes(),
				);

				// Add ScoreKey
				let score_key = ScoreKey::new(key.clone(), score, member);
				batch.put_cf(db.cf_handle(&cf_name).unwrap(), score_key.encode(), []);
			}
		}
		self.db().write_opt(batch, &Default::default())?;

		if added_count > 0 {
			meta_val.len += added_count;
			self.db_put(&meta_encoded_key, &meta_val.encode()).await?;
		}

		Ok(added_count)
	}

	pub async fn zrange(
		&self,
		key: Bytes,
		start: isize,
		stop: isize,
		with_scores: bool,
	) -> Result<Vec<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
		let Some(meta) = self.get_valid_zset_meta(&key).await? else {
			return Ok(Vec::new());
		};

		// Adjust indices
		let len = meta.len as isize;
		let start = if start < 0 { len + start } else { start };
		let stop = if stop < 0 { len + stop } else { stop };

		if start < 0 || start >= len || start > stop {
			return Ok(Vec::new());
		}

		// Create prefix for score keys: key_prefix + b'S'
		let key_prefix = Self::create_key_prefix(&key);
		let mut prefix = Vec::with_capacity(key_prefix.len() + 1);
		prefix.extend_from_slice(&key_prefix);
		prefix.push(b'S');

		let results = self.scan_with_prefix("zset", &prefix).await?;

		let mut result = Vec::new();
		// Cache header length and offset to avoid repeated calculation
		let header_len = 2 + key.len() + 1 + 8;
		let score_offset = 2 + key.len() + 1;

		// Convert to usize since enumerate() produces usize indices
		let start_usize = start as usize;
		let stop_usize = stop as usize;

		for (current_idx, (k, _v)) in results.into_iter().enumerate() {
			let k_bytes: Bytes = k.into();
			if !k_bytes.starts_with(&prefix) {
				break;
			}

			if current_idx >= start_usize && current_idx <= stop_usize {
				// Extract member and score
				// Key: len(user_key) + user_key + b'S' + score (8 bytes) + member
				if k_bytes.len() > header_len {
					let member = k_bytes.slice(header_len..);
					result.push(member);
					if with_scores {
						let score_bytes: [u8; 8] =
							k_bytes[score_offset..score_offset + 8].try_into().unwrap();
						let encoded_score = u64::from_be_bytes(score_bytes);
						let score = ScoreKey::decode_score(encoded_score);
						let score_str = score.to_string();
						result.push(Bytes::copy_from_slice(score_str.as_bytes()));
					}
				}
			}

			if current_idx > stop_usize {
				break;
			}
		}

		Ok(result)
	}

	pub async fn zscore(
		&self,
		key: Bytes,
		member: Bytes,
	) -> Result<Option<f64>, Box<dyn std::error::Error + Send + Sync>> {
		if self.get_valid_zset_meta(&key).await?.is_none() {
			return Ok(None);
		}

		let member_key = MemberKey::new(key, member);
		if let Some(val) = self.zset_get(member_key.encode().to_vec()).await? {
			// Val is encoded score (u64 BE)
			let encoded_score = u64::from_be_bytes(val[..8].try_into()?);
			Ok(Some(ScoreKey::decode_score(encoded_score)))
		} else {
			Ok(None)
		}
	}

	pub async fn zrem(
		&self,
		key: Bytes,
		members: Vec<Bytes>,
	) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = meta_key.encode();

		let mut meta_val = match self.get_valid_zset_meta(&key).await? {
			Some(val) => val,
			None => return Ok(0),
		};

		let mut removed_count = 0;

		// Batch pre-fetch all member keys to avoid N individual I/O calls
		let mut member_encoded_keys = Vec::with_capacity(members.len());
		for member in &members {
			let member_key = MemberKey::new(key.clone(), member.clone());
			member_encoded_keys.push(member_key.encode());
		}

		let mut batch_fetches = Vec::new();
		for encoded_key in &member_encoded_keys {
			batch_fetches.push(self.zset_get(encoded_key.to_vec()));
		}

		let mut old_values = Vec::new();
		for fetch in batch_fetches {
			old_values.push(fetch.await?);
		}

		// Use WriteBatch to ensure atomicity of all delete operations
		let mut batch = WriteBatch::default();
		let cf_name = "zset".to_string();
		let db = self.db().clone();

		for (idx, member) in members.into_iter().enumerate() {
			let encoded_member_key = &member_encoded_keys[idx];
			if let Some(val) = &old_values[idx] {
				// Delete MemberKey
				batch.delete_cf(db.cf_handle(&cf_name).unwrap(), encoded_member_key.clone());

				// Delete ScoreKey
				let encoded_score = u64::from_be_bytes(val[..8].try_into()?);
				let score = ScoreKey::decode_score(encoded_score);
				let score_key = ScoreKey::new(key.clone(), score, member);
				batch.delete_cf(db.cf_handle(&cf_name).unwrap(), score_key.encode());

				removed_count += 1;
			}
		}

		// Execute all delete operations atomically
		self.db().write_opt(batch, &Default::default())?;

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

	pub async fn zcard(&self, key: Bytes) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		if let Some(meta_val) = self.get_valid_zset_meta(&key).await? {
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
		let path = std::env::temp_dir().join(format!("nimbis_test_zset_{}", timestamp));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path).await.unwrap();
		(storage, path)
	}

	#[tokio::test]
	async fn test_zadd_zrange() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("myzset");

		let added = storage
			.zadd(
				key.clone(),
				vec![
					(1.0, Bytes::from("one")),
					(2.0, Bytes::from("two")),
					(3.0, Bytes::from("three")),
				],
			)
			.await
			.unwrap();
		assert_eq!(added, 3);

		// Update score
		let added = storage
			.zadd(key.clone(), vec![(5.0, Bytes::from("two"))])
			.await
			.unwrap();
		assert_eq!(added, 0); // No new element

		let members = storage.zrange(key.clone(), 0, -1, false).await.unwrap();
		assert_eq!(members.len(), 3);
		assert_eq!(members[0], Bytes::from("one"));
		assert_eq!(members[1], Bytes::from("three"));
		assert_eq!(members[2], Bytes::from("two"));

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_zscore() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("myzset");

		storage
			.zadd(key.clone(), vec![(1.5, Bytes::from("one"))])
			.await
			.unwrap();

		let score = storage
			.zscore(key.clone(), Bytes::from("one"))
			.await
			.unwrap();
		assert_eq!(score, Some(1.5));

		let score = storage
			.zscore(key.clone(), Bytes::from("missing"))
			.await
			.unwrap();
		assert_eq!(score, None);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_zrem() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("myzset");

		storage
			.zadd(
				key.clone(),
				vec![(1.0, Bytes::from("one")), (2.0, Bytes::from("two"))],
			)
			.await
			.unwrap();

		let removed = storage
			.zrem(key.clone(), vec![Bytes::from("one")])
			.await
			.unwrap();
		assert_eq!(removed, 1);

		let members = storage.zrange(key.clone(), 0, -1, false).await.unwrap();
		assert_eq!(members.len(), 1);
		assert_eq!(members[0], Bytes::from("two"));

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_zset_collision_repro() {
		let (storage, path) = get_storage().await;
		let key1 = Bytes::from("user1");

		// 1. Add to user1
		storage
			.zadd(key1.clone(), vec![(1.0, Bytes::from("m1"))])
			.await
			.unwrap();

		// 2. Simulate FLUSHDB
		storage.flush_all().await.unwrap();

		// 3. Re-Add to user1
		storage
			.zadd(key1.clone(), vec![(1.0, Bytes::from("m1"))])
			.await
			.unwrap();

		// 4. ZCard user1
		let card = storage.zcard(key1.clone()).await.unwrap();
		assert_eq!(card, 1, "ZCard user1 should be 1");

		// 5. ZRange user1
		let members = storage.zrange(key1.clone(), 0, -1, false).await.unwrap();
		assert_eq!(members.len(), 1, "ZRange user1 should have 1 member");
		assert_eq!(members[0], Bytes::from("m1"));

		let _ = std::fs::remove_dir_all(path);
	}
}
