use bytes::Bytes;
use futures::future;
use log::warn;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;

use crate::error::StorageError;
use crate::expirable::Expirable;
use crate::list::element_key::ListElementKey;
use crate::storage::Storage;
use crate::string::meta::ListMetaValue;
use crate::string::meta::MetaKey;

impl Storage {
	pub async fn lpush(&self, key: Bytes, elements: Vec<Bytes>) -> Result<u64, StorageError> {
		self.list_push(key, elements, true).await
	}

	pub async fn rpush(&self, key: Bytes, elements: Vec<Bytes>) -> Result<u64, StorageError> {
		self.list_push(key, elements, false).await
	}

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
		let meta_encoded_key = meta_key.encode();

		let mut meta_val = match self.get_meta::<ListMetaValue>(&key).await? {
			Some(m) => m,
			None => ListMetaValue::new(self.version_generator.next()),
		};

		let write_opts = WriteOptions {
			await_durable: false,
		};
		let put_opts = PutOptions::default();

		for element in elements {
			let seq = if is_left {
				meta_val.head -= 1;
				meta_val.head
			} else {
				let s = meta_val.tail;
				meta_val.tail += 1;
				s
			};

			let element_key = ListElementKey::new(key.clone(), meta_val.version, seq);
			self.list_db
				.put_with_options(element_key.encode(), element, &put_opts, &write_opts)
				.await?;
			meta_val.len += 1;
		}

		// Update metadata
		// Preserve TTL if it exists
		let ttl = meta_val
			.remaining_ttl()
			.map(|d| d.as_millis() as u64)
			.map(slatedb::config::Ttl::ExpireAfter)
			.unwrap_or(slatedb::config::Ttl::NoExpiry);

		let meta_put_opts = PutOptions { ttl };

		self.string_db
			.put_with_options(
				meta_encoded_key,
				meta_val.encode(),
				&meta_put_opts,
				&write_opts,
			)
			.await?;

		Ok(meta_val.len)
	}

	pub async fn lpop(&self, key: Bytes, count: Option<usize>) -> Result<Vec<Bytes>, StorageError> {
		self.list_pop(key, count, true).await
	}

	pub async fn rpop(&self, key: Bytes, count: Option<usize>) -> Result<Vec<Bytes>, StorageError> {
		self.list_pop(key, count, false).await
	}

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
		let write_opts = WriteOptions {
			await_durable: false,
		};

		// We will pop up to `num` elements
		let loop_count = std::cmp::min(num as u64, meta_val.len);

		for _ in 0..loop_count {
			let seq = if is_left {
				meta_val.head
			} else {
				meta_val.tail - 1
			};

			let element_key = ListElementKey::new(key.clone(), meta_val.version, seq);
			// Get element
			if let Some(val) = self.list_db.get(element_key.encode()).await? {
				results.push(val);

				// Only update meta and delete if element exists
				if is_left {
					meta_val.head += 1;
				} else {
					meta_val.tail -= 1;
				}
				meta_val.len -= 1;

				self.list_db
					.delete_with_options(element_key.encode(), &write_opts)
					.await?;
			}
		}

		// Update metadata
		let meta_key = MetaKey::new(key.clone());

		if meta_val.len == 0 {
			// List empty, delete metadata
			self.string_db
				.delete_with_options(meta_key.encode(), &write_opts)
				.await?;
		} else {
			let ttl = meta_val
				.remaining_ttl()
				.map(|d| d.as_millis() as u64)
				.map(slatedb::config::Ttl::ExpireAfter)
				.unwrap_or(slatedb::config::Ttl::NoExpiry);

			let meta_put_opts = PutOptions { ttl };

			self.string_db
				.put_with_options(
					meta_key.encode(),
					meta_val.encode(),
					&meta_put_opts,
					&write_opts,
				)
				.await?;
		}

		Ok(results)
	}

	pub async fn llen(&self, key: Bytes) -> Result<u64, StorageError> {
		if let Some(meta_val) = self.get_meta::<ListMetaValue>(&key).await? {
			Ok(meta_val.len)
		} else {
			Ok(0)
		}
	}

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
				let element_key = ListElementKey::new(key.clone(), meta_val.version, seq);
				async move { self.list_db.get(element_key.encode()).await }
			})
			.collect();

		let found_results = future::try_join_all(futures).await?;

		for res in found_results {
			if let Some(val) = res {
				results.push(val);
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
}
