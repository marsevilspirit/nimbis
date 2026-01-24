use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use slatedb::Db;
use slatedb::object_store::ObjectStore;
use slatedb::object_store::local::LocalFileSystem;

use crate::data_type::DataType;
use crate::error::StorageError;
use crate::string::meta::MetaKey;
use crate::string::meta::MetaValue;

#[derive(Clone)]
pub struct Storage {
	pub(crate) string_db: Arc<Db>,
	pub(crate) hash_db: Arc<Db>,
	pub(crate) list_db: Arc<Db>,
	pub(crate) set_db: Arc<Db>,
	pub(crate) zset_db: Arc<Db>,
}

impl Storage {
	pub fn new(
		string_db: Arc<Db>,
		hash_db: Arc<Db>,
		list_db: Arc<Db>,
		set_db: Arc<Db>,
		zset_db: Arc<Db>,
	) -> Self {
		Self {
			string_db,
			hash_db,
			list_db,
			set_db,
			zset_db,
		}
	}

	pub async fn open(
		path: impl AsRef<Path>,
		shard_id: Option<usize>,
	) -> Result<Self, StorageError> {
		let mut path_buf = path.as_ref().to_path_buf();
		if let Some(id) = shard_id {
			path_buf.push(format!("shard-{}", id));
		}
		let path = path_buf.as_path();

		// Ensure shard directory exists
		tokio::fs::create_dir_all(path).await?;

		let object_store: Arc<dyn ObjectStore> = Arc::new(LocalFileSystem::new_with_prefix(path)?);

		let open_db = |name: &'static str| {
			let store = object_store.clone();
			async move { Db::open(slatedb::object_store::path::Path::from(name), store).await }
		};

		let (string_db, hash_db, list_db, set_db, zset_db) = tokio::try_join!(
			open_db("/string"),
			open_db("/hash"),
			open_db("/list"),
			open_db("/set"),
			open_db("/zset")
		)?;

		Ok(Self::new(
			Arc::new(string_db),
			Arc::new(hash_db),
			Arc::new(list_db),
			Arc::new(set_db),
			Arc::new(zset_db),
		))
	}

	pub async fn flush_all(&self) -> Result<(), StorageError> {
		// Iterate over all DBs and delete all keys
		// Since we don't have atomic flush_all, we do best effort sequential
		// Scanning and deleting everything is slow but correct for tests.
		// For production this is blocking and bad, but it's FLUSHDB.

		// Helper to clear a DB
		async fn clear_db(
			db: &slatedb::Db,
		) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
			let scan_range = ..;
			let mut stream = db.scan::<bytes::Bytes, _>(scan_range).await?;
			let write_opts = slatedb::config::WriteOptions {
				await_durable: false,
			};
			while let Some(kv) = stream.next().await? {
				db.delete_with_options(kv.key, &write_opts).await?;
			}
			Ok(())
		}

		clear_db(&self.string_db).await?;
		clear_db(&self.hash_db).await?;
		clear_db(&self.list_db).await?;
		clear_db(&self.set_db).await?;
		clear_db(&self.zset_db).await?;

		Ok(())
	}

	/// Helper to get and validate metadata for any collection type.
	/// Returns:
	/// - Ok(Some(meta)) if the key is a valid, non-expired meta of type T
	/// - Ok(None) if the key doesn't exist or is expired
	/// - Err if the key exists but is of wrong type
	pub(crate) async fn get_meta<T: MetaValue>(
		&self,
		key: &Bytes,
	) -> Result<Option<T>, StorageError> {
		let meta_key = MetaKey::new(key.clone());
		let meta_bytes = match self.string_db.get(meta_key.encode()).await? {
			Some(bytes) => bytes,
			None => return Ok(None),
		};

		if meta_bytes.is_empty() {
			return Ok(None);
		}

		let actual_type_u8 = meta_bytes[0];
		if !T::is_type_match(actual_type_u8) {
			return Err(StorageError::WrongType {
				expected: None,
				actual: DataType::from_u8(actual_type_u8).unwrap_or(DataType::String),
			});
		}

		let meta_val = T::decode(&meta_bytes)?;
		if meta_val.is_expired() {
			self.del(key.clone()).await?;
			return Ok(None);
		}

		Ok(Some(meta_val))
	}

	/// Helper to delete all keys starting with a given prefix from a specific
	/// database.
	pub(crate) async fn delete_keys_by_prefix(
		&self,
		db: &Arc<Db>,
		prefix: Bytes,
	) -> Result<(), StorageError> {
		use slatedb::config::WriteOptions;

		let range = prefix.clone()..;
		let mut stream = db.scan(range).await?;
		let mut keys_to_delete = Vec::new();

		while let Some(kv) = stream.next().await? {
			if !kv.key.starts_with(&prefix) {
				break;
			}
			keys_to_delete.push(kv.key);
		}

		if !keys_to_delete.is_empty() {
			let write_opts = WriteOptions {
				await_durable: false,
			};
			for k in keys_to_delete {
				db.delete_with_options(k, &write_opts).await?;
			}
		}

		Ok(())
	}
}
