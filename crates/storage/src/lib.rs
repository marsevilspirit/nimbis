use async_trait::async_trait;
use bytes::Bytes;

use slatedb::Db;
use slatedb::object_store::{ObjectStore, local::LocalFileSystem};
use std::path::Path;
use std::sync::Arc;

#[async_trait]
pub trait Storage: Send + Sync {
    /// Get value by key
    async fn get(
        &self,
        key: &str,
    ) -> Result<Option<Bytes>, Box<dyn std::error::Error + Send + Sync>>;

    /// Set value for key
    async fn set(
        &self,
        key: &str,
        value: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

#[derive(Clone)]
pub struct ObjectStorage {
    db: Arc<Db>,
}

impl ObjectStorage {
    /// Open a new SlateDB storage backed by local file system
    pub async fn open(
        path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // TODO: configure SlateDB options
        let object_store: Arc<dyn ObjectStore> = Arc::new(LocalFileSystem::new_with_prefix(path)?);
        // let options = DbOptions::default();
        // Assuming open takes (path, object_store) based on lint error
        // Or (path, options, object_store) ? No, "expected 2 arguments".
        // Maybe (path, options) and options contains object_store? (Unlikely since error said found DbOptions expected ObjectStore).
        // Maybe (object_store, options)?
        // Wait, lint error said "expected Arc<ObjectStore> found DbOptions" at 2nd arg.
        // So 2nd arg is ObjectStore.
        // 1st arg is Path.
        // So Db::open(path, object_store).
        let db_path = slatedb::object_store::path::Path::from("/");
        let db = Db::open(db_path, object_store).await?;

        Ok(Self { db: Arc::new(db) })
    }
}

#[async_trait]
impl Storage for ObjectStorage {
    async fn get(
        &self,
        key: &str,
    ) -> Result<Option<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
        let result = self.db.get(key.as_bytes()).await?;
        Ok(result)
    }

    async fn set(
        &self,
        key: &str,
        value: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.db.put(key.as_bytes(), value.as_bytes()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn test_object_storage() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!("nimbis_test_{}", timestamp));
        std::fs::create_dir_all(&path)?;
        let storage = ObjectStorage::open(&path).await?;

        // Test set and get
        storage.set("key1", "value1").await?;
        let result = storage.get("key1").await?;
        assert_eq!(result, Some(Bytes::from("value1")));

        // Test missing key
        let missing = storage.get("missing").await?;
        assert_eq!(missing, None);

        // Test overwrite
        storage.set("key1", "new_value").await?;
        let result = storage.get("key1").await?;
        assert_eq!(result, Some(Bytes::from("new_value")));

        // Clean up (best effort)
        let _ = std::fs::remove_dir_all(path);

        Ok(())
    }
}
