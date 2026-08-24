use bytes::Bytes;
use futures::future;
use log::warn;
use nimbis_macros::storage_lock;
use slatedb::WriteBatch;
use slatedb::config::PutOptions;

use crate::data_type::DataType;
use crate::error::StorageError;
use crate::list::element_key::ListElementKey;
use crate::storage::Storage;
use crate::string::meta::ListMetaValue;
use crate::typed_db::MetadataChange;

impl Storage {
	fn validate_list_meta(meta: &ListMetaValue) -> Result<(), StorageError> {
		if meta.len == 0 {
			return Err(StorageError::DataInconsistency {
				message: "stored list metadata has zero length".to_string(),
			});
		}

		let span =
			meta.tail
				.checked_sub(meta.head)
				.ok_or_else(|| StorageError::DataInconsistency {
					message: "list metadata tail is before head".to_string(),
				})?;
		if span != meta.len {
			return Err(StorageError::DataInconsistency {
				message: format!(
					"list metadata length mismatch: len={}, head={}, tail={}",
					meta.len, meta.head, meta.tail
				),
			});
		}

		Ok(())
	}

	#[fastrace::trace]
	pub async fn lpush(&self, key: Bytes, elements: Vec<Bytes>) -> Result<u64, StorageError> {
		self.list_push(key, elements, true).await
	}

	#[fastrace::trace]
	pub async fn rpush(&self, key: Bytes, elements: Vec<Bytes>) -> Result<u64, StorageError> {
		self.list_push(key, elements, false).await
	}

	#[storage_lock(write, key, DataType::List)]
	async fn list_push(
		&self,
		key: Bytes,
		elements: Vec<Bytes>,
		is_left: bool,
	) -> Result<u64, StorageError> {
		if elements.is_empty() {
			// If key exists, return len. If not, return 0.
			if let Some(meta) = self.list_db.load(&key).await? {
				Self::validate_list_meta(&meta)?;
				return Ok(meta.len);
			} else {
				return Ok(0);
			}
		}

		let put_opts = PutOptions::default();

		let mut meta_val = match self.list_db.load(&key).await? {
			Some(meta) => {
				Self::validate_list_meta(&meta)?;
				meta
			}
			None => ListMetaValue::new(0),
		};
		let mut batch = WriteBatch::new();

		for element in elements {
			meta_val.len =
				meta_val
					.len
					.checked_add(1)
					.ok_or_else(|| StorageError::DataInconsistency {
						message: "list length overflow during push".to_string(),
					})?;
			let seq = if is_left {
				meta_val.head = meta_val.head.checked_sub(1).ok_or_else(|| {
					StorageError::DataInconsistency {
						message: "list head underflow during LPUSH".to_string(),
					}
				})?;
				meta_val.head
			} else {
				let s = meta_val.tail;
				meta_val.tail = meta_val.tail.checked_add(1).ok_or_else(|| {
					StorageError::DataInconsistency {
						message: "list tail overflow during RPUSH".to_string(),
					}
				})?;
				s
			};

			let element_key = ListElementKey::new(key.clone(), seq);
			batch.put_with_options(element_key.encode()?, element, &put_opts);
		}

		let new_len = meta_val.len;
		self.list_db
			.commit(&key, batch, MetadataChange::Put(meta_val))
			.await?;

		Ok(new_len)
	}

	#[fastrace::trace]
	pub async fn lpop(&self, key: Bytes, count: Option<usize>) -> Result<Vec<Bytes>, StorageError> {
		self.list_pop(key, count, true).await
	}

	#[fastrace::trace]
	pub async fn rpop(&self, key: Bytes, count: Option<usize>) -> Result<Vec<Bytes>, StorageError> {
		self.list_pop(key, count, false).await
	}

	#[storage_lock(write, key, DataType::List)]
	async fn list_pop(
		&self,
		key: Bytes,
		count: Option<usize>,
		is_left: bool,
	) -> Result<Vec<Bytes>, StorageError> {
		let Some(mut meta_val) = self.list_db.load(&key).await? else {
			return Ok(Vec::new());
		};
		Self::validate_list_meta(&meta_val)?;

		let num = count.unwrap_or(1);
		if num == 0 {
			return Ok(Vec::new());
		}

		// We will pop up to `num` elements
		let requested = u64::try_from(num).map_err(|_| StorageError::DataInconsistency {
			message: "list pop count does not fit in u64".to_string(),
		})?;
		let loop_count = std::cmp::min(requested, meta_val.len);
		let result_capacity =
			usize::try_from(loop_count).map_err(|_| StorageError::DataInconsistency {
				message: "list pop result does not fit in memory".to_string(),
			})?;
		let mut results = Vec::with_capacity(result_capacity);
		let mut encoded_element_keys = Vec::with_capacity(result_capacity);

		for _ in 0..loop_count {
			let seq = if is_left {
				meta_val.head
			} else {
				meta_val
					.tail
					.checked_sub(1)
					.ok_or_else(|| StorageError::DataInconsistency {
						message: "list tail underflow during RPOP".to_string(),
					})?
			};

			let element_key = ListElementKey::new(key.clone(), seq);
			let encoded_element_key = element_key.encode()?;
			let Some(kv) = self
				.list_db
				.get_entry(encoded_element_key.clone())
				.await?
				.filter(|kv| kv.seq >= meta_val.version)
			else {
				return Err(StorageError::DataInconsistency {
					message: format!(
						"list element missing or stale for key {key:?} at logical sequence {seq}"
					),
				});
			};

			results.push(kv.value);
			encoded_element_keys.push(encoded_element_key);
			if is_left {
				meta_val.head = meta_val.head.checked_add(1).ok_or_else(|| {
					StorageError::DataInconsistency {
						message: "list head overflow during LPOP".to_string(),
					}
				})?;
			} else {
				meta_val.tail = seq;
			}
			meta_val.len =
				meta_val
					.len
					.checked_sub(1)
					.ok_or_else(|| StorageError::DataInconsistency {
						message: "list length underflow during pop".to_string(),
					})?;
		}

		let mut batch = WriteBatch::new();
		for encoded_element_key in encoded_element_keys {
			batch.delete(encoded_element_key);
		}
		let metadata = if meta_val.len == 0 {
			MetadataChange::Delete
		} else {
			MetadataChange::Put(meta_val)
		};
		self.list_db.commit(&key, batch, metadata).await?;

		Ok(results)
	}

	#[storage_lock(read, key, DataType::List)]
	#[fastrace::trace]
	pub async fn llen(&self, key: Bytes) -> Result<u64, StorageError> {
		if let Some(meta_val) = self.list_db.load(&key).await? {
			Self::validate_list_meta(&meta_val)?;
			Ok(meta_val.len)
		} else {
			Ok(0)
		}
	}

	#[storage_lock(read, key, DataType::List)]
	#[fastrace::trace]
	pub async fn lrange(
		&self,
		key: Bytes,
		start: i64,
		stop: i64,
	) -> Result<Vec<Bytes>, StorageError> {
		let Some(meta_val) = self.list_db.load(&key).await? else {
			return Ok(Vec::new());
		};
		Self::validate_list_meta(&meta_val)?;

		// Normalize indices
		let len = meta_val.len as i64;
		let start_idx = if start < 0 { len + start } else { start };
		let stop_idx = if stop < 0 { len + stop } else { stop };

		// Clamp
		let start_idx = std::cmp::max(0, start_idx);
		let stop_idx = std::cmp::min(len - 1, stop_idx);

		if start_idx > stop_idx {
			return Ok(Vec::new());
		}

		// range size
		let count = (stop_idx - start_idx + 1) as usize;
		let mut results = Vec::with_capacity(count);

		// Calculate actual sequences
		// Sequences are [head, tail).
		// 0-th element is at head.
		// i-th element is at head + i.

		let start_seq = meta_val.head + start_idx as u64;
		let stop_seq = meta_val.head + stop_idx as u64;

		// We use parallel GETs to fetch elements since we know the exact sequence
		// numbers. Ranges are contiguous, so we can iterate from start_seq to
		// stop_seq. TODO: Consider using scan for potentially better performance on
		// large ranges, though simple GETs are sufficient given the sequence number
		// design.

		let futures: Vec<_> = (start_seq..=stop_seq)
			.map(|seq| {
				let element_key = ListElementKey::new(key.clone(), seq);
				async move { self.list_db.get_entry(element_key.encode()?).await }
			})
			.collect();

		let found_results = future::try_join_all(futures).await?;

		for res in found_results {
			if let Some(kv) = res
				&& kv.seq >= meta_val.version
			{
				results.push(kv.value);
			} else {
				// Should not happen if consistency is maintained
				warn!(
					"List element missing for key {:?} at sequence. Potential data inconsistency.",
					key
				);
			}
		}

		Ok(results)
	}
}

#[cfg(test)]
mod tests {
	use slatedb::config::WriteOptions;

	use super::*;
	use crate::data_type::DataType;
	use crate::string::meta::ListMetaValue;
	use crate::top_level_key::TopLevelKey;
	use crate::typed_db::metadata_put_options;

	fn metric<V>(db: &crate::typed_db::TypedDb<V>, name: &'static str) -> i64 {
		db.metric(name)
	}

	async fn get_storage() -> (Storage, std::path::PathBuf) {
		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_test_list_{}", timestamp));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		(storage, path)
	}

	#[tokio::test]
	async fn test_lpush_lpop() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("mylist");

		// LPUSH
		let len = storage
			.lpush(key.clone(), vec![Bytes::from("v1"), Bytes::from("v2")])
			.await
			.unwrap();
		assert_eq!(len, 2);

		// Structure: v2, v1
		// LPOP
		let popped = storage.lpop(key.clone(), None).await.unwrap();
		assert_eq!(popped.len(), 1);
		assert_eq!(popped[0], Bytes::from("v2"));

		// LPOP remaining
		let popped = storage.lpop(key.clone(), None).await.unwrap();
		assert_eq!(popped[0], Bytes::from("v1"));

		// Empty
		let len = storage.llen(key.clone()).await.unwrap();
		assert_eq!(len, 0);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_rpush_rpop() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("mylist_r");

		// RPUSH
		let len = storage
			.rpush(key.clone(), vec![Bytes::from("v1"), Bytes::from("v2")])
			.await
			.unwrap();
		assert_eq!(len, 2);

		// Structure: v1, v2
		// RPOP
		let popped = storage.rpop(key.clone(), None).await.unwrap();
		assert_eq!(popped[0], Bytes::from("v2"));

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_lrange() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("mylist_range");

		// Push 1, 2, 3 so list is [1, 2, 3]
		storage
			.rpush(
				key.clone(),
				vec![Bytes::from("1"), Bytes::from("2"), Bytes::from("3")],
			)
			.await
			.unwrap();

		let all = storage.lrange(key.clone(), 0, -1).await.unwrap();
		assert_eq!(all.len(), 3);
		assert_eq!(all[0], Bytes::from("1"));
		assert_eq!(all[2], Bytes::from("3"));

		let part = storage.lrange(key.clone(), 0, 1).await.unwrap();
		assert_eq!(part.len(), 2);
		assert_eq!(part[1], Bytes::from("2"));

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_list_metadata_and_elements_share_one_list_batch() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("atomic_list_layout");
		let meta_key = TopLevelKey::new(key.clone()).unwrap().encode();
		let before_list_batches = metric(&storage.list_db, "slatedb.db.write_batch_count");
		let before_list_ops = metric(&storage.list_db, "slatedb.db.write_ops");
		let before_string_batches = metric(&storage.string_db, "slatedb.db.write_batch_count");

		let len = storage
			.rpush(
				key.clone(),
				vec![Bytes::from("first"), Bytes::from("second")],
			)
			.await
			.unwrap();
		assert_eq!(len, 2);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_batch_count") - before_list_batches,
			1
		);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_ops") - before_list_ops,
			3
		);
		assert_eq!(
			metric(&storage.string_db, "slatedb.db.write_batch_count") - before_string_batches,
			0
		);

		let raw_meta = storage
			.list_db
			.raw()
			.get_key_value(meta_key.clone())
			.await
			.unwrap()
			.expect("list metadata should live in list_db");
		let encoded_meta = ListMetaValue::decode(&raw_meta.value).unwrap();
		assert_eq!(encoded_meta.version, 0, "zero marks the batch generation");
		assert_eq!(encoded_meta.len, 2);

		for seq in encoded_meta.head..encoded_meta.tail {
			let raw_element = storage
				.list_db
				.raw()
				.get_key_value(ListElementKey::new(key.clone(), seq).encode().unwrap())
				.await
				.unwrap()
				.expect("list element should exist in the same batch");
			assert_eq!(raw_element.seq, raw_meta.seq);
		}

		let resolved_meta = storage.list_db.load(&key).await.unwrap().unwrap();
		assert_eq!(resolved_meta.version, raw_meta.seq);
		assert!(
			storage
				.string_db
				.raw()
				.get(meta_key)
				.await
				.unwrap()
				.is_none()
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_list_pop_batches_deletes_and_metadata_update() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("atomic_list_pop");
		storage
			.rpush(
				key.clone(),
				vec![Bytes::from("dup"), Bytes::from("dup"), Bytes::from("tail")],
			)
			.await
			.unwrap();

		let before_batches = metric(&storage.list_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.list_db, "slatedb.db.write_ops");
		let popped = storage.lpop(key.clone(), Some(2)).await.unwrap();
		assert_eq!(popped, vec![Bytes::from("dup"), Bytes::from("dup")]);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_batch_count") - before_batches,
			1
		);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_ops") - before_ops,
			3
		);
		assert_eq!(storage.llen(key.clone()).await.unwrap(), 1);

		let before_batches = metric(&storage.list_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.list_db, "slatedb.db.write_ops");
		assert_eq!(
			storage.rpop(key.clone(), None).await.unwrap(),
			vec![Bytes::from("tail")]
		);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_batch_count") - before_batches,
			1
		);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_ops") - before_ops,
			2
		);
		assert!(
			storage
				.list_db
				.raw()
				.get(TopLevelKey::new(key).unwrap().encode())
				.await
				.unwrap()
				.is_none()
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_empty_push_and_zero_count_pop_do_not_write() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("list_zero_write");
		let before_batches = metric(&storage.list_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.list_db, "slatedb.db.write_ops");
		assert_eq!(storage.rpush(key.clone(), Vec::new()).await.unwrap(), 0);
		assert!(storage.lpop(key.clone(), Some(0)).await.unwrap().is_empty());
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_batch_count") - before_batches,
			0
		);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_ops") - before_ops,
			0
		);

		storage
			.rpush(key.clone(), vec![Bytes::from("value")])
			.await
			.unwrap();
		let before_batches = metric(&storage.list_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.list_db, "slatedb.db.write_ops");
		assert_eq!(storage.lpush(key.clone(), Vec::new()).await.unwrap(), 1);
		assert!(storage.rpop(key, Some(0)).await.unwrap().is_empty());
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_batch_count") - before_batches,
			0
		);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_ops") - before_ops,
			0
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_list_pop_missing_element_leaves_all_visible_state_unchanged() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("list_pop_inconsistent");
		storage
			.rpush(
				key.clone(),
				vec![Bytes::from("first"), Bytes::from("missing")],
			)
			.await
			.unwrap();

		let meta_key = TopLevelKey::new(key.clone()).unwrap().encode();
		let raw_meta_before = storage
			.list_db
			.raw()
			.get_key_value(meta_key.clone())
			.await
			.unwrap()
			.unwrap();
		let meta = storage.list_db.load(&key).await.unwrap().unwrap();
		let first_key = ListElementKey::new(key.clone(), meta.head)
			.encode()
			.unwrap();
		let missing_key = ListElementKey::new(key.clone(), meta.head + 1)
			.encode()
			.unwrap();
		storage.list_db.raw().delete(missing_key).await.unwrap();

		let before_batches = metric(&storage.list_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.list_db, "slatedb.db.write_ops");
		let err = storage.lpop(key.clone(), Some(2)).await.unwrap_err();
		assert!(matches!(err, StorageError::DataInconsistency { .. }));
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_batch_count") - before_batches,
			0
		);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_ops") - before_ops,
			0
		);

		let raw_meta_after = storage
			.list_db
			.raw()
			.get_key_value(meta_key)
			.await
			.unwrap()
			.unwrap();
		assert_eq!(raw_meta_after.seq, raw_meta_before.seq);
		assert_eq!(raw_meta_after.value, raw_meta_before.value);
		assert_eq!(
			storage.list_db.raw().get(first_key).await.unwrap(),
			Some(Bytes::from("first"))
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_list_push_checked_arithmetic_fails_before_write() {
		let (storage, path) = get_storage().await;
		let write_opts = WriteOptions::default();
		let put_opts = PutOptions::default();

		let head_key = Bytes::from("list_head_underflow");
		let mut head_meta = ListMetaValue::new(1);
		head_meta.head = 0;
		head_meta.tail = 1;
		head_meta.len = 1;
		storage
			.list_db
			.raw()
			.put_with_options(
				TopLevelKey::new(head_key.clone()).unwrap().encode(),
				head_meta.encode(),
				&put_opts,
				&write_opts,
			)
			.await
			.unwrap();

		let len_key = Bytes::from("list_length_overflow");
		let mut len_meta = ListMetaValue::new(1);
		len_meta.head = 0;
		len_meta.tail = u64::MAX;
		len_meta.len = u64::MAX;
		storage
			.list_db
			.raw()
			.put_with_options(
				TopLevelKey::new(len_key.clone()).unwrap().encode(),
				len_meta.encode(),
				&put_opts,
				&write_opts,
			)
			.await
			.unwrap();

		let before_batches = metric(&storage.list_db, "slatedb.db.write_batch_count");
		let before_ops = metric(&storage.list_db, "slatedb.db.write_ops");
		assert!(
			storage
				.lpush(head_key, vec![Bytes::from("value")])
				.await
				.is_err()
		);
		assert!(
			storage
				.rpush(len_key, vec![Bytes::from("value")])
				.await
				.is_err()
		);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_batch_count") - before_batches,
			0
		);
		assert_eq!(
			metric(&storage.list_db, "slatedb.db.write_ops") - before_ops,
			0
		);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_list_push_preserves_metadata_ttl_and_generation() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("list_ttl_generation");
		let meta_key = TopLevelKey::new(key.clone()).unwrap().encode();
		storage
			.rpush(key.clone(), vec![Bytes::from("first")])
			.await
			.unwrap();

		let mut meta = storage.list_db.load(&key).await.unwrap().unwrap();
		let generation = meta.version;
		meta.expire_time =
			(chrono::Utc::now().timestamp_millis().max(0) as u64).saturating_add(60_000);
		let put_opts = metadata_put_options(&meta).unwrap();
		let write_opts = WriteOptions::default();
		storage
			.list_db
			.raw()
			.put_with_options(meta_key.clone(), meta.encode(), &put_opts, &write_opts)
			.await
			.unwrap();
		let expire_before = storage
			.list_db
			.raw()
			.get_key_value(meta_key.clone())
			.await
			.unwrap()
			.unwrap()
			.expire_ts
			.unwrap();

		storage
			.rpush(key.clone(), vec![Bytes::from("second")])
			.await
			.unwrap();
		let raw_meta_after = storage
			.list_db
			.raw()
			.get_key_value(meta_key)
			.await
			.unwrap()
			.unwrap();
		let expire_after = raw_meta_after.expire_ts.unwrap();
		assert!(expire_after > chrono::Utc::now().timestamp_millis());
		assert!(expire_after.abs_diff(expire_before) < 1_000);
		let meta_after = ListMetaValue::decode(&raw_meta_after.value).unwrap();
		assert_eq!(meta_after.version, generation);
		assert_eq!(meta_after.len, 2);

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_list_version_init_stable_and_recreate() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("list_version_lifecycle");

		let len = storage
			.rpush(key.clone(), vec![Bytes::from("a"), Bytes::from("b")])
			.await
			.unwrap();
		assert_eq!(len, 2);

		let version_v1 = storage
			.list_db
			.raw()
			.get_key_value(TopLevelKey::new(key.clone()).unwrap().encode())
			.await
			.unwrap()
			.unwrap()
			.seq;

		let len = storage
			.rpush(key.clone(), vec![Bytes::from("c")])
			.await
			.unwrap();
		assert_eq!(len, 3);

		let version_after_push = storage
			.list_db
			.raw()
			.get_key_value(TopLevelKey::new(key.clone()).unwrap().encode())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(
			ListMetaValue::decode(&version_after_push.value)
				.unwrap()
				.version,
			version_v1
		);

		let popped = storage.lpop(key.clone(), None).await.unwrap();
		assert_eq!(popped, vec![Bytes::from("a")]);

		let version_after_pop = storage
			.list_db
			.raw()
			.get_key_value(TopLevelKey::new(key.clone()).unwrap().encode())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(
			ListMetaValue::decode(&version_after_pop.value)
				.unwrap()
				.version,
			version_v1
		);

		assert_eq!(storage.del(DataType::List, [key.clone()]).await.unwrap(), 1);

		let len = storage
			.rpush(key.clone(), vec![Bytes::from("x")])
			.await
			.unwrap();
		assert_eq!(len, 1);

		let version_v2 = storage
			.list_db
			.raw()
			.get_key_value(TopLevelKey::new(key.clone()).unwrap().encode())
			.await
			.unwrap()
			.unwrap()
			.seq;
		assert!(version_v2 > version_v1);

		let elems = storage.lrange(key.clone(), 0, -1).await.unwrap();
		assert_eq!(elems, vec![Bytes::from("x")]);

		let _ = std::fs::remove_dir_all(path);
	}
}
