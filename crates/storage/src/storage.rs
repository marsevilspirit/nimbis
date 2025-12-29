use slatedb::Db;
use slatedb::object_store::{ObjectStore, local::LocalFileSystem};
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub struct Storage {
    pub(crate) db: Arc<Db>,
}

impl Storage {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    /// Open a new SlateDB storage backed by local file system
    pub async fn open(
        path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let object_store: Arc<dyn ObjectStore> = Arc::new(LocalFileSystem::new_with_prefix(path)?);
        let db_path = slatedb::object_store::path::Path::from("/");
        let db = Db::open(db_path, object_store).await?;
        Ok(Self::new(Arc::new(db)))
    }
}
