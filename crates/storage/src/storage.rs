use std::path::Path;
use std::sync::Arc;

use rocksdb::ColumnFamilyDescriptor;
use rocksdb::DB;
use rocksdb::Options;
use rocksdb::ReadOptions;
use rocksdb::WriteBatch;
use rocksdb::WriteOptions;

#[derive(Clone)]
pub struct Storage {
	db: Arc<DB>,
}

impl Storage {
	pub fn new(db: Arc<DB>) -> Self {
		Self { db }
	}

	pub async fn open(
		path: impl AsRef<Path>,
	) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
		let path = path.as_ref();

		// Create column families
		let cf_names = ["hash", "list", "set", "zset"];

		// Open in a blocking task since RocksDB is synchronous
		let path_buf = path.to_path_buf();
		let db = tokio::task::spawn_blocking(move || {
			let mut opts = Options::default();
			opts.create_if_missing(true);
			opts.create_missing_column_families(true);

			// Create column family descriptors
			let cf_descriptors: Vec<ColumnFamilyDescriptor> = cf_names
				.iter()
				.map(|name| ColumnFamilyDescriptor::new(*name, Options::default()))
				.collect();

			DB::open_cf_descriptors(&opts, &path_buf, cf_descriptors)
		})
		.await??;

		Ok(Self::new(Arc::new(db)))
	}

	/// Common method to get a value from any column family
	async fn cf_get(
		&self,
		cf: &str,
		key: &[u8],
	) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let key_vec = key.to_vec();
		let cf_name = cf.to_string();
		let result = tokio::task::spawn_blocking(move || {
			db.get_cf(db.cf_handle(&cf_name).unwrap(), &key_vec)
		})
		.await??;
		Ok(result)
	}

	/// Common method to put a value to any column family
	async fn cf_put(
		&self,
		cf: &str,
		key: &[u8],
		value: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let key_vec = key.to_vec();
		let value_vec = value.to_vec();
		let cf_name = cf.to_string();
		tokio::task::spawn_blocking(move || {
			db.put_cf(db.cf_handle(&cf_name).unwrap(), &key_vec, &value_vec)
		})
		.await??;
		Ok(())
	}

	/// Common method to delete a key from any column family
	async fn cf_delete(
		&self,
		cf: &str,
		key: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let key_vec = key.to_vec();
		let cf_name = cf.to_string();
		tokio::task::spawn_blocking(move || {
			db.delete_cf(db.cf_handle(&cf_name).unwrap(), &key_vec)
		})
		.await??;
		Ok(())
	}

	/// Get a value from default column family
	pub async fn db_get(
		&self,
		key: &[u8],
	) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let key_vec = key.to_vec();
		let result = tokio::task::spawn_blocking(move || db.get(&key_vec)).await??;
		Ok(result)
	}

	/// Put a value to default column family
	pub async fn db_put(
		&self,
		key: &[u8],
		value: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let key_vec = key.to_vec();
		let value_vec = value.to_vec();
		tokio::task::spawn_blocking(move || db.put(&key_vec, &value_vec)).await??;
		Ok(())
	}

	/// Delete a key from default column family
	pub async fn db_delete(
		&self,
		key: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let key_vec = key.to_vec();
		tokio::task::spawn_blocking(move || db.delete(&key_vec)).await??;
		Ok(())
	}

	/// Get a value from hash column family
	pub async fn hash_get(
		&self,
		key: &[u8],
	) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
		self.cf_get("hash", key).await
	}

	/// Put a value to hash column family
	pub async fn hash_put(
		&self,
		key: &[u8],
		value: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.cf_put("hash", key, value).await
	}

	/// Delete a key from hash column family
	pub async fn hash_delete(
		&self,
		key: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.cf_delete("hash", key).await
	}

	/// Get a value from list column family
	pub async fn list_get(
		&self,
		key: &[u8],
	) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
		self.cf_get("list", key).await
	}

	/// Put a value to list column family
	pub async fn list_put(
		&self,
		key: &[u8],
		value: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.cf_put("list", key, value).await
	}

	/// Delete a key from list column family
	pub async fn list_delete(
		&self,
		key: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.cf_delete("list", key).await
	}

	/// Get a value from set column family
	pub async fn set_get(
		&self,
		key: &[u8],
	) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
		self.cf_get("set", key).await
	}

	/// Put a value to set column family
	pub async fn set_put(
		&self,
		key: &[u8],
		value: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.cf_put("set", key, value).await
	}

	/// Delete a key from set column family
	pub async fn set_delete(
		&self,
		key: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.cf_delete("set", key).await
	}

	/// Get a value from zset column family
	pub async fn zset_get(
		&self,
		key: Vec<u8>,
	) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let result =
			tokio::task::spawn_blocking(move || db.get_cf(db.cf_handle("zset").unwrap(), &key))
				.await??;
		Ok(result)
	}

	/// Put a value to zset column family
	pub async fn zset_put(
		&self,
		key: &[u8],
		value: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.cf_put("zset", key, value).await
	}

	/// Delete a key from zset column family
	pub async fn zset_delete(
		&self,
		key: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		self.cf_delete("zset", key).await
	}

	/// Create a key prefix: len(key) + key (length prefixed to avoid prefix collisions)
	pub fn create_key_prefix(key: &[u8]) -> Vec<u8> {
		let mut prefix = Vec::with_capacity(2 + key.len());
		prefix.extend_from_slice(&(key.len() as u16).to_be_bytes());
		prefix.extend_from_slice(key);
		prefix
	}

	/// Get prefix iterator bounds for scanning with a prefix
	fn get_prefix_bounds(prefix: &[u8]) -> (Vec<u8>, Vec<u8>) {
		let mut upper = prefix.to_vec();
		if let Some(last) = upper.last_mut() {
			*last = last.wrapping_add(1);
		}
		(prefix.to_vec(), upper)
	}

	/// Scan keys with a prefix from default column family
	pub async fn scan_with_prefix_default(
		&self,
		prefix: &[u8],
	) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let prefix_vec = prefix.to_vec();

		tokio::task::spawn_blocking(move || {
			let mut read_opts = ReadOptions::default();
			let (_, upper) = Self::get_prefix_bounds(&prefix_vec);
			read_opts.set_iterate_upper_bound(upper);

			let mut results = Vec::new();
			let iter = db.iterator_opt(
				rocksdb::IteratorMode::From(&prefix_vec, rocksdb::Direction::Forward),
				read_opts,
			);

			for item in iter {
				let (key, value) = item?;
				results.push((key.to_vec(), value.to_vec()));
			}

			Ok(results)
		})
		.await?
	}

	/// Scan keys with a prefix from a column family
	pub async fn scan_with_prefix(
		&self,
		cf_name: &str,
		prefix: &[u8],
	) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let prefix_vec = prefix.to_vec();
		let cf_name = cf_name.to_string();

		tokio::task::spawn_blocking(move || {
			let cf = db.cf_handle(&cf_name).unwrap();
			let mut read_opts = ReadOptions::default();
			let (_, upper) = Self::get_prefix_bounds(&prefix_vec);
			read_opts.set_iterate_upper_bound(upper);

			let mut results = Vec::new();
			let iter = db.iterator_cf_opt(
				cf,
				read_opts,
				rocksdb::IteratorMode::From(&prefix_vec, rocksdb::Direction::Forward),
			);

			for item in iter {
				let (key, value) = item?;
				results.push((key.to_vec(), value.to_vec()));
			}

			Ok(results)
		})
		.await?
	}

	/// Delete all keys with a prefix from default column family
	pub async fn delete_with_prefix_default(
		&self,
		prefix: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let prefix_vec = prefix.to_vec();

		tokio::task::spawn_blocking(move || {
			let mut read_opts = ReadOptions::default();
			let (_, upper) = Self::get_prefix_bounds(&prefix_vec);
			read_opts.set_iterate_upper_bound(upper);

			let mut batch = WriteBatch::default();
			let iter = db.iterator_opt(
				rocksdb::IteratorMode::From(&prefix_vec, rocksdb::Direction::Forward),
				read_opts,
			);

			for item in iter {
				let (key, _) = item?;
				batch.delete(&key);
			}

			if !batch.is_empty() {
				let mut write_opts = WriteOptions::default();
				write_opts.set_sync(false);
				db.write_opt(batch, &write_opts)?;
			}

			Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
		})
		.await??;

		Ok(())
	}

	/// Delete all keys with a prefix from a column family
	pub async fn delete_with_prefix(
		&self,
		cf_name: &str,
		prefix: &[u8],
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();
		let prefix_vec = prefix.to_vec();
		let cf_name = cf_name.to_string();

		tokio::task::spawn_blocking(move || {
			let cf = db.cf_handle(&cf_name).unwrap();
			let mut read_opts = ReadOptions::default();
			let (_, upper) = Self::get_prefix_bounds(&prefix_vec);
			read_opts.set_iterate_upper_bound(upper);

			let mut batch = WriteBatch::default();
			let iter = db.iterator_cf_opt(
				cf,
				read_opts,
				rocksdb::IteratorMode::From(&prefix_vec, rocksdb::Direction::Forward),
			);

			for item in iter {
				let (key, _) = item?;
				batch.delete_cf(cf, &key);
			}

			if !batch.is_empty() {
				let mut write_opts = WriteOptions::default();
				write_opts.set_sync(false);
				db.write_opt(batch, &write_opts)?;
			}

			Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
		})
		.await??;

		Ok(())
	}

	pub async fn flush_all(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let db = self.db.clone();

		tokio::task::spawn_blocking(move || {
			// Helper to clear a column family or default CF
			let clear_cf =
				|cf_name: Option<&str>| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
					let mut keys_to_delete = Vec::new();

					if let Some(name) = cf_name {
						let cf_handle = db
							.cf_handle(name)
							.ok_or_else(|| format!("Column family {} not found", name))?;

						let iter = db.iterator_cf(&cf_handle, rocksdb::IteratorMode::Start);
						for item in iter {
							let (key, _) = item?;
							keys_to_delete.push(key.to_vec());
						}

						for key in keys_to_delete {
							db.delete_cf(&cf_handle, key)?;
						}
					} else {
						// Default CF
						let iter = db.iterator(rocksdb::IteratorMode::Start);
						for item in iter {
							let (key, _) = item?;
							keys_to_delete.push(key.to_vec());
						}

						for key in keys_to_delete {
							db.delete(key)?;
						}
					}

					Ok(())
				};

			// Clear all column families
			clear_cf(None)?; // Default CF (string/meta)
			clear_cf(Some("hash"))?;
			clear_cf(Some("list"))?;
			clear_cf(Some("set"))?;
			clear_cf(Some("zset"))?;

			Ok(())
		})
		.await?
	}

	// Expose the DB for internal use
	pub(crate) fn db(&self) -> &Arc<DB> {
		&self.db
	}
}
