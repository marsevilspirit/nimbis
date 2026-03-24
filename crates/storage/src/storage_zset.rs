use bytes::Bytes;
use futures::future;
use slatedb::WriteBatch;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;

use crate::error::StorageError;
use crate::storage::Storage;
use crate::string::meta::MetaKey;
use crate::string::meta::ZSetMetaValue;
use crate::util::zset_score_user_key_prefix;
use crate::zset::member_key::MemberKey;
use crate::zset::score_key::ScoreKey;

impl Storage {
	pub async fn zadd(
		&self,
		key: Bytes,
		elements: Vec<(f64, Bytes)>, // (score, member)
	) -> Result<u64, StorageError> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = meta_key.encode();
		let write_opts = WriteOptions {
			await_durable: false,
		};
		let put_opts = PutOptions::default();

		// Get metadata first to obtain version
		let (mut meta_val, meta_missing) = match self.get_meta::<ZSetMetaValue>(&key).await? {
			Some(val) => (val, false),
			None => (ZSetMetaValue::new(0, 0), true),
		};

		// Prepare member fetch futures using version from metadata
		let mut member_encoded_keys = Vec::with_capacity(elements.len());
		let mut member_futs = Vec::with_capacity(elements.len());
		for (_, member) in &elements {
			let member_key = MemberKey::new(key.clone(), member.clone());
			let enc = member_key.encode();
			member_encoded_keys.push(enc.clone());
			member_futs.push(self.get_entry(&self.zset_db, enc));
		}

		// Fetch all members in parallel
		let members_res = future::join_all(member_futs).await;

		let old_values: Vec<_> = if meta_missing {
			vec![None; elements.len()]
		} else {
			members_res
				.into_iter()
				.collect::<Result<Vec<_>, _>>()?
				.into_iter()
				.map(|entry| match entry {
					Some(kv) if kv.seq >= meta_val.version => Some(kv.value),
					_ => None,
				})
				.collect()
		};

		let mut added_count = 0;
		let mut first_new_member_key: Option<Bytes> = None;
		// Use WriteBatch to ensure atomicity of all zset operations
		let mut batch = WriteBatch::new();
		let mut has_writes = false;

		for (idx, (score, member)) in elements.into_iter().enumerate() {
			let encoded_member_key = &member_encoded_keys[idx];
			let old_score_bytes = &old_values[idx];

			if let Some(old_score_bytes) = old_score_bytes {
				// Update existing member
				let old_score =
					ScoreKey::decode_score(u64::from_be_bytes(old_score_bytes[..8].try_into()?));
				if old_score != score {
					has_writes = true;
					// Delete old ScoreKey
					let old_score_key = ScoreKey::new(key.clone(), old_score, member.clone());
					batch.delete(old_score_key.encode());

					// Add new ScoreKey
					let new_score_key = ScoreKey::new(key.clone(), score, member.clone());
					batch.put_with_options(new_score_key.encode(), Bytes::new(), &put_opts);

					// Update MemberKey
					let encoded_score = ScoreKey::encode_score(score);
					batch.put_with_options(
						encoded_member_key.clone(),
						Bytes::copy_from_slice(&encoded_score.to_be_bytes()),
						&put_opts,
					);
				}
			} else {
				has_writes = true;
				// New member
				added_count += 1;
				if first_new_member_key.is_none() {
					first_new_member_key = Some(encoded_member_key.clone());
				}

				// Add MemberKey
				let encoded_score = ScoreKey::encode_score(score);
				batch.put_with_options(
					encoded_member_key.clone(),
					Bytes::copy_from_slice(&encoded_score.to_be_bytes()),
					&put_opts,
				);

				// Add ScoreKey
				let score_key = ScoreKey::new(key.clone(), score, member);
				batch.put_with_options(score_key.encode(), Bytes::new(), &put_opts);
			}
		}

		if has_writes {
			self.zset_db.write_with_options(batch, &write_opts).await?;
		}

		if meta_missing && added_count > 0 {
			let Some(first_key) = first_new_member_key else {
				return Err(StorageError::DataInconsistency {
					message: "missing first new zset member key after write".to_string(),
				});
			};
			let first_entry = self
				.get_entry(&self.zset_db, first_key)
				.await?
				.ok_or_else(|| StorageError::DataInconsistency {
					message: "failed to read first new zset member after write".to_string(),
				})?;
			meta_val.version = first_entry.seq;
		}

		if added_count > 0 {
			meta_val.len += added_count;

			let put_opts = PutOptions::default();

			self.string_db
				.put_with_options(meta_encoded_key, meta_val.encode(), &put_opts, &write_opts)
				.await?;
		}

		Ok(added_count)
	}

	pub async fn zrange(
		&self,
		key: Bytes,
		start: isize,
		stop: isize,
		with_scores: bool,
	) -> Result<Vec<Bytes>, StorageError> {
		if let Some(meta) = self.get_meta::<ZSetMetaValue>(&key).await? {
			// Adjust indices
			let len = meta.len as isize;
			let start = if start < 0 { len + start } else { start };
			let stop = if stop < 0 { len + stop } else { stop };

			if start < 0 || start >= len || start > stop {
				return Ok(Vec::new());
			}

			// We need to scan ScoreKeys.
			// Key format: len(user_key) + user_key + b'S' + score + member
			let prefix = zset_score_user_key_prefix(&key);

			let range = prefix.as_ref()..;
			let mut stream = self.zset_db.scan(range).await?;

			let mut result = Vec::new();
			let mut current_idx = 0;
			// Cache header length and offset to avoid repeated calculation
			// prefix = key_len(2) + key + b'S'(1), then score(8) + member
			let header_len = prefix.len() + 8; // prefix + score(8)
			let score_offset = prefix.len(); // score starts right after prefix

			while let Some(kv) = stream.next().await? {
				let k = kv.key;
				if !k.starts_with(&prefix) {
					break;
				}
				if kv.seq < meta.version {
					continue;
				}

				if current_idx >= start && current_idx <= stop {
					// Extract member and score
					// Key: len(user_key) + user_key + b'S' + score(8) + member
					if k.len() > header_len {
						let member = k.slice(header_len..);
						result.push(member);
						if with_scores {
							let score_bytes: [u8; 8] =
								k[score_offset..score_offset + 8].try_into()?;
							let encoded_score = u64::from_be_bytes(score_bytes);
							let score = ScoreKey::decode_score(encoded_score);
							let score_str = score.to_string();
							result.push(Bytes::copy_from_slice(score_str.as_bytes()));
						}
					}
				}

				if current_idx > stop {
					break;
				}
				current_idx += 1;
			}
			Ok(result)
		} else {
			Ok(Vec::new())
		}
	}

	pub async fn zscore(&self, key: Bytes, member: Bytes) -> Result<Option<f64>, StorageError> {
		let Some(meta_val) = self.get_meta::<ZSetMetaValue>(&key).await? else {
			return Ok(None);
		};

		let member_key = MemberKey::new(key, member);
		if let Some(entry) = self.get_entry(&self.zset_db, member_key.encode()).await?
			&& entry.seq >= meta_val.version
		{
			// Val is encoded score (u64 BE)
			let encoded_score = u64::from_be_bytes(entry.value[..8].try_into()?);
			Ok(Some(ScoreKey::decode_score(encoded_score)))
		} else {
			Ok(None)
		}
	}

	pub async fn zrem(&self, key: Bytes, members: Vec<Bytes>) -> Result<u64, StorageError> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = meta_key.encode();

		let mut meta_val = match self.get_meta::<ZSetMetaValue>(&key).await? {
			Some(val) => val,
			None => return Ok(0),
		};

		// Fetch all member keys in parallel
		let mut member_encoded_keys = Vec::with_capacity(members.len());
		let fetch_futures = members.iter().map(|member| {
			let member_key = MemberKey::new(key.clone(), member.clone());
			let encoded_key = member_key.encode();
			member_encoded_keys.push(encoded_key.clone());
			self.get_entry(&self.zset_db, encoded_key)
		});

		let old_values: Result<Vec<_>, _> =
			future::join_all(fetch_futures).await.into_iter().collect();
		let old_values = old_values?
			.into_iter()
			.map(|entry| match entry {
				Some(kv) if kv.seq >= meta_val.version => Some(kv.value),
				_ => None,
			})
			.collect::<Vec<_>>();

		// Use WriteBatch to ensure atomicity of all delete operations
		let mut batch = WriteBatch::new();
		let mut removed_count = 0;

		for (idx, member) in members.into_iter().enumerate() {
			if let Some(val) = &old_values[idx] {
				// Delete MemberKey
				batch.delete(&member_encoded_keys[idx]);

				// Delete ScoreKey
				let encoded_score = u64::from_be_bytes(val[..8].try_into()?);
				let score = ScoreKey::decode_score(encoded_score);
				let score_key = ScoreKey::new(key.clone(), score, member);
				batch.delete(score_key.encode());

				removed_count += 1;
			}
		}

		if removed_count == 0 {
			return Ok(0);
		}

		let write_opts = WriteOptions {
			await_durable: false,
		};

		// Execute all delete operations atomically
		self.zset_db.write_with_options(batch, &write_opts).await?;

		meta_val.len -= removed_count;
		if meta_val.len == 0 {
			self.string_db
				.delete_with_options(meta_encoded_key, &write_opts)
				.await?;
		} else {
			let put_opts = PutOptions::default();
			self.string_db
				.put_with_options(meta_encoded_key, meta_val.encode(), &put_opts, &write_opts)
				.await?;
		}

		Ok(removed_count)
	}

	pub async fn zcard(&self, key: Bytes) -> Result<u64, StorageError> {
		if let Some(meta_val) = self.get_meta::<ZSetMetaValue>(&key).await? {
			Ok(meta_val.len)
		} else {
			Ok(0)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::string::meta::ZSetMetaValue;

	async fn get_storage() -> (Storage, std::path::PathBuf) {
		let timestamp = ulid::Ulid::new().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_test_zset_{}", timestamp));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
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

		// Add to user1
		storage
			.zadd(key1.clone(), vec![(1.0, Bytes::from("m1"))])
			.await
			.unwrap();

		// Simulate FLUSHDB
		storage.flush_all().await.unwrap();

		// Re-Add to user1
		storage
			.zadd(key1.clone(), vec![(1.0, Bytes::from("m1"))])
			.await
			.unwrap();

		// ZCard user1
		let card = storage.zcard(key1.clone()).await.unwrap();
		assert_eq!(card, 1, "ZCard user1 should be 1");

		// ZRange user1
		let members = storage.zrange(key1.clone(), 0, -1, false).await.unwrap();
		assert_eq!(members.len(), 1, "ZRange user1 should have 1 member");
		assert_eq!(members[0], Bytes::from("m1"));

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_zset_version_init_stable_and_recreate() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("zset_version_lifecycle");

		let added = storage
			.zadd(key.clone(), vec![(1.0, Bytes::from("m1"))])
			.await
			.unwrap();
		assert_eq!(added, 1);

		let version_v1 = storage
			.get_meta::<ZSetMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;

		let added = storage
			.zadd(key.clone(), vec![(2.0, Bytes::from("m1"))])
			.await
			.unwrap();
		assert_eq!(added, 0);

		let version_after_score_update = storage
			.get_meta::<ZSetMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;
		assert_eq!(version_after_score_update, version_v1);

		let added = storage
			.zadd(key.clone(), vec![(3.0, Bytes::from("m2"))])
			.await
			.unwrap();
		assert_eq!(added, 1);

		let version_after_new_member = storage
			.get_meta::<ZSetMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;
		assert_eq!(version_after_new_member, version_v1);

		let deleted = storage.del(key.clone()).await.unwrap();
		assert!(deleted);

		let added = storage
			.zadd(key.clone(), vec![(10.0, Bytes::from("m1"))])
			.await
			.unwrap();
		assert_eq!(added, 1);

		let version_v2 = storage
			.get_meta::<ZSetMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;
		assert!(version_v2 > version_v1);

		let members = storage.zrange(key.clone(), 0, -1, false).await.unwrap();
		assert_eq!(members, vec![Bytes::from("m1")]);

		let _ = std::fs::remove_dir_all(path);
	}
}
