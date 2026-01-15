use std::path::Path;
use std::sync::Arc;

use slatedb::Db;
use slatedb::object_store::ObjectStore;
use slatedb::object_store::local::LocalFileSystem;

use crate::lock_manager::LockManager;

#[derive(Clone)]
pub struct Storage {
	pub(crate) string_db: Arc<Db>,
	pub(crate) hash_db: Arc<Db>,
	pub(crate) list_db: Arc<Db>,
	pub(crate) set_db: Arc<Db>,
	pub(crate) zset_db: Arc<Db>,
	pub lock_manager: Arc<LockManager>,
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
			lock_manager: Arc::new(LockManager::new()),
		}
	}

	pub async fn open(
		path: impl AsRef<Path>,
	) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
		let path = path.as_ref();
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

		let storage = Self::new(
			Arc::new(string_db),
			Arc::new(hash_db),
			Arc::new(list_db),
			Arc::new(set_db),
			Arc::new(zset_db),
		);
		Ok(storage)
	}

	pub async fn flush_all(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
}
