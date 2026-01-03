use bytes::Bytes;
use futures::future;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;

use crate::data_type::DataType;
use crate::expirable::Expirable;
use crate::list::element_key::ListElementKey;
use crate::storage::Storage;
use crate::string::meta::ListMetaValue;
use crate::string::meta::MetaKey;

impl Storage {
	// Helper to get and validate list metadata.
	// Returns:
	// - Ok(Some(meta)) if the key is a valid, non-expired List
	// - Ok(None) if the key doesn't exist or is expired
	// - Err if the key exists but is of wrong type
	async fn get_valid_list_meta(
		&self,
		key: &Bytes,
	) -> Result<Option<ListMetaValue>, Box<dyn std::error::Error + Send + Sync>> {
		let meta_key = MetaKey::new(key.clone());
		if let Some(meta_bytes) = self.string_db.get(meta_key.encode()).await? {
			if meta_bytes.is_empty() {
				return Ok(None);
			}
			if meta_bytes[0] != DataType::List as u8 {
				return Err(
					"WRONGTYPE Operation against a key holding the wrong kind of value".into(),
				);
			}
			let meta_val = ListMetaValue::decode(&meta_bytes)?;
			if meta_val.is_expired() {
				self.del(key.clone()).await?;
				return Ok(None);
			}
			Ok(Some(meta_val))
		} else {
			Ok(None)
		}
	}

	// Helper to delete all elements of a list.
	pub(crate) async fn delete_list_elements(
		&self,
		key: Bytes,
		meta: &ListMetaValue,
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let write_opts = WriteOptions {
			await_durable: false,
		};
		// Elements are stored in the range [head, tail).
		// head points to the first element, and tail points to one past the last element.
		// We iterate through this range to delete all individual elements.

		for i in meta.head..meta.tail {
			let field_key = ListElementKey::new(key.clone(), i);
			self.list_db
				.delete_with_options(field_key.encode(), &write_opts)
				.await?;
		}
		Ok(())
	}

	pub async fn lpush(
		&self,
		key: Bytes,
		elements: Vec<Bytes>,
	) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		self.list_push(key, elements, true).await
	}

	pub async fn rpush(
		&self,
		key: Bytes,
		elements: Vec<Bytes>,
	) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		self.list_push(key, elements, false).await
	}

	async fn list_push(
		&self,
		key: Bytes,
		elements: Vec<Bytes>,
		is_left: bool,
	) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		if elements.is_empty() {
			// If key exists, return len. If not, return 0.
			if let Some(meta) = self.get_valid_list_meta(&key).await? {
				return Ok(meta.len);
			} else {
				return Ok(0);
			}
		}

		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = meta_key.encode();

		// Check type and get current meta
		let current_meta_bytes = self.string_db.get(meta_encoded_key.clone()).await?;

		let mut meta_val = if let Some(meta_bytes) = current_meta_bytes {
			if meta_bytes.is_empty() {
				ListMetaValue::new()
			} else {
				match DataType::from_u8(meta_bytes[0]) {
					Some(DataType::String)
					| Some(DataType::Hash)
					| Some(DataType::Set)
					| Some(DataType::ZSet) => {
						return Err(
							"WRONGTYPE Operation against a key holding the wrong kind of value"
								.into(),
						);
					}
					Some(DataType::List) => {
						let val = ListMetaValue::decode(&meta_bytes)?;
						if val.is_expired() {
							self.delete_list_elements(key.clone(), &val).await?;
							ListMetaValue::new()
						} else {
							val
						}
					}
					None => ListMetaValue::new(),
				}
			}
		} else {
			ListMetaValue::new()
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

			let element_key = ListElementKey::new(key.clone(), seq);
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

	pub async fn lpop(
		&self,
		key: Bytes,
		count: Option<usize>,
	) -> Result<Vec<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
		self.list_pop(key, count, true).await
	}

	pub async fn rpop(
		&self,
		key: Bytes,
		count: Option<usize>,
	) -> Result<Vec<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
		self.list_pop(key, count, false).await
	}

	async fn list_pop(
		&self,
		key: Bytes,
		count: Option<usize>,
		is_left: bool,
	) -> Result<Vec<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
		let Some(mut meta_val) = self.get_valid_list_meta(&key).await? else {
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
				let s = meta_val.head;
				meta_val.head += 1;
				s
			} else {
				meta_val.tail -= 1;
				meta_val.tail
			};

			let element_key = ListElementKey::new(key.clone(), seq);
			// Get element
			if let Some(val) = self.list_db.get(element_key.encode()).await? {
				results.push(val);
			}

			// Delete element from list_db
			self.list_db
				.delete_with_options(element_key.encode(), &write_opts)
				.await?;

			meta_val.len -= 1;
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

	pub async fn llen(&self, key: Bytes) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
		if let Some(meta_val) = self.get_valid_list_meta(&key).await? {
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
	) -> Result<Vec<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
		let Some(meta_val) = self.get_valid_list_meta(&key).await? else {
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

		// We use parallel GETs to fetch elements since we know the exact sequence numbers.
		// Ranges are contiguous, so we can iterate from start_seq to stop_seq.
		// TODO: Consider using scan for potentially better performance on large ranges,
		// though simple GETs are sufficient given the sequence number design.

		let futures: Vec<_> = (start_seq..=stop_seq)
			.map(|seq| {
				let element_key = ListElementKey::new(key.clone(), seq);
				async move { self.list_db.get(element_key.encode()).await }
			})
			.collect();

		let found_results = future::try_join_all(futures).await?;

		for res in found_results {
			if let Some(val) = res {
				results.push(val);
			} else {
				// Should not happen if consistency is maintained
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
		let storage = Storage::open(&path).await.unwrap();
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
