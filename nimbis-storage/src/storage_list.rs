use bytes::Bytes;
use futures::future;
use log::warn;
use nimbis_macros::storage_lock;
use slatedb::WriteBatch;
use slatedb::config::PutOptions;

use crate::error::StorageError;
use crate::list::element_key::ListElementKey;
use crate::segment::Segment;
use crate::storage::Storage;
use crate::string::meta::ListMetaValue;
use crate::string::meta::MetaKey;

impl Storage {
	#[fastrace::trace]
	pub async fn lpush(&self, key: Bytes, elements: Vec<Bytes>) -> Result<u64, StorageError> {
		self.list_push(key, elements, true).await
	}

	#[fastrace::trace]
	pub async fn rpush(&self, key: Bytes, elements: Vec<Bytes>) -> Result<u64, StorageError> {
		self.list_push(key, elements, false).await
	}

	#[storage_lock(write, key)]
	async fn list_push(
		&self,
		key: Bytes,
		elements: Vec<Bytes>,
		is_left: bool,
	) -> Result<u64, StorageError> {
		if elements.is_empty() {
			// If key exists, return len. If not, return 0.
			if let Some(meta) = self.get_meta::<ListMetaValue>(&key).await? {
				return Ok(meta.len);
			} else {
				return Ok(0);
			}
		}

		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = Segment::Meta.wrap(meta_key.encode());
		let put_opts = PutOptions::default();

		let (mut meta_val, meta_missing) = match self.get_meta::<ListMetaValue>(&key).await? {
			Some(m) => (m, false),
			None => (ListMetaValue::new(0), true),
		};
		let mut batch = WriteBatch::new();

		for element in elements {
			let seq = if is_left {
				meta_val.head -= 1;
				meta_val.head
			} else {
				let s = meta_val.tail;
				meta_val.tail += 1;
				s
			};

			let element_key = ListElementKey::new(key.clone(), seq);
			batch.put_with_options(Segment::List.wrap(element_key.encode()), element, &put_opts);
			meta_val.len += 1;
		}

		self.write_batch_with_seq(|seq| {
			if meta_missing {
				meta_val.version = seq;
			}

			// Update metadata
			let meta_put_opts = Storage::meta_put_opts(&meta_val);

			batch.put_with_options(meta_encoded_key, meta_val.encode(), &meta_put_opts);
			batch
		})
		.await?;

		Ok(meta_val.len)
	}

	#[fastrace::trace]
	pub async fn lpop(&self, key: Bytes, count: Option<usize>) -> Result<Vec<Bytes>, StorageError> {
		self.list_pop(key, count, true).await
	}

	#[fastrace::trace]
	pub async fn rpop(&self, key: Bytes, count: Option<usize>) -> Result<Vec<Bytes>, StorageError> {
		self.list_pop(key, count, false).await
	}

	#[storage_lock(write, key)]
	async fn list_pop(
		&self,
		key: Bytes,
		count: Option<usize>,
		is_left: bool,
	) -> Result<Vec<Bytes>, StorageError> {
		let Some(mut meta_val) = self.get_meta::<ListMetaValue>(&key).await? else {
			return Ok(Vec::new());
		};

		let num = count.unwrap_or(1);
		if num == 0 {
			return Ok(Vec::new());
		}

		let mut results = Vec::with_capacity(num);
		let mut batch = WriteBatch::new();

		// We will pop up to `num` elements
		let loop_count = std::cmp::min(num as u64, meta_val.len);

		for _ in 0..loop_count {
			let seq = if is_left {
				meta_val.head
			} else {
				meta_val.tail - 1
			};

			let element_key = ListElementKey::new(key.clone(), seq);
			let encoded_key = Segment::List.wrap(element_key.encode());
			// Get element
			if let Some(val) = self
				.db
				.get_key_value(encoded_key.clone())
				.await?
				.filter(|kv| kv.seq >= meta_val.version)
				.map(|kv| kv.value)
			{
				results.push(val);

				// Only update meta and delete if element exists
				if is_left {
					meta_val.head += 1;
				} else {
					meta_val.tail -= 1;
				}
				meta_val.len -= 1;

				batch.delete(encoded_key);
			}
		}

		// Update metadata
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = Segment::Meta.wrap(meta_key.encode());

		if meta_val.len == 0 {
			// List empty, delete metadata
			batch.delete(meta_encoded_key);
		} else {
			let meta_put_opts = Storage::meta_put_opts(&meta_val);

			batch.put_with_options(meta_encoded_key, meta_val.encode(), &meta_put_opts);
		}

		if !results.is_empty() {
			self.write_batch(batch).await?;
		}

		Ok(results)
	}

	#[storage_lock(read, key)]
	#[fastrace::trace]
	pub async fn llen(&self, key: Bytes) -> Result<u64, StorageError> {
		if let Some(meta_val) = self.get_meta::<ListMetaValue>(&key).await? {
			Ok(meta_val.len)
		} else {
			Ok(0)
		}
	}

	#[storage_lock(read, key)]
	#[fastrace::trace]
	pub async fn lrange(
		&self,
		key: Bytes,
		start: i64,
		stop: i64,
	) -> Result<Vec<Bytes>, StorageError> {
		let Some(meta_val) = self.get_meta::<ListMetaValue>(&key).await? else {
			return Ok(Vec::new());
		};

		if meta_val.len == 0 {
			return Ok(Vec::new());
		}

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
				async move {
					self.db
						.get_key_value(Segment::List.wrap(element_key.encode()))
						.await
						.map_err(StorageError::from)
				}
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
	use super::*;
	use crate::string::meta::ListMetaValue;

	async fn get_storage() -> (Storage, std::path::PathBuf) {
		let timestamp = ulid::Ulid::new().to_string();
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
	async fn test_list_version_init_stable_and_recreate() {
		let (storage, path) = get_storage().await;
		let key = Bytes::from("list_version_lifecycle");

		let len = storage
			.rpush(key.clone(), vec![Bytes::from("a"), Bytes::from("b")])
			.await
			.unwrap();
		assert_eq!(len, 2);

		let version_v1 = storage
			.get_meta::<ListMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;

		let len = storage
			.rpush(key.clone(), vec![Bytes::from("c")])
			.await
			.unwrap();
		assert_eq!(len, 3);

		let version_after_push = storage
			.get_meta::<ListMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;
		assert_eq!(version_after_push, version_v1);

		let popped = storage.lpop(key.clone(), None).await.unwrap();
		assert_eq!(popped, vec![Bytes::from("a")]);

		let version_after_pop = storage
			.get_meta::<ListMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;
		assert_eq!(version_after_pop, version_v1);

		let deleted = storage.del([key.clone()]).await.unwrap();
		assert_eq!(deleted, 1);

		let len = storage
			.rpush(key.clone(), vec![Bytes::from("x")])
			.await
			.unwrap();
		assert_eq!(len, 1);

		let version_v2 = storage
			.get_meta::<ListMetaValue>(&key)
			.await
			.unwrap()
			.unwrap()
			.version;
		assert!(version_v2 > version_v1);

		let elems = storage.lrange(key.clone(), 0, -1).await.unwrap();
		assert_eq!(elems, vec![Bytes::from("x")]);

		let _ = std::fs::remove_dir_all(path);
	}
}
