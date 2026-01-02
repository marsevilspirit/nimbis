use std::path::Path;
use std::sync::Arc;

use slatedb::Db;
use slatedb::object_store::ObjectStore;
use slatedb::object_store::local::LocalFileSystem;

#[derive(Clone)]
pub struct Storage {
	pub(crate) string_db: Arc<Db>,
	pub(crate) hash_db: Arc<Db>,
	// TODO: add more type db
}

impl Storage {
	pub fn new(string_db: Arc<Db>, hash_db: Arc<Db>) -> Self {
		Self { string_db, hash_db }
	}

	/// Open a new SlateDB storage backed by local file system
	pub async fn open(
		path: impl AsRef<Path>,
	) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
		let path = path.as_ref();
		let object_store: Arc<dyn ObjectStore> = Arc::new(LocalFileSystem::new_with_prefix(path)?);

		let string_db = Db::open(
			slatedb::object_store::path::Path::from("/string"),
			object_store.clone(),
		)
		.await?;
		let hash_db = Db::open(
			slatedb::object_store::path::Path::from("/hash"),
			object_store.clone(),
		)
		.await?;

		Ok(Self::new(Arc::new(string_db), Arc::new(hash_db)))
	}
}
