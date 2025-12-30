use crate::storage::Storage;
use crate::string::key::StringKey;
use crate::string::value::StringValue;
use bytes::Bytes;

impl Storage {
    pub async fn get(
        &self,
        key: Bytes,
    ) -> Result<Option<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
        let key = StringKey::new(key);
        let result = self.db.get(key.encode()).await?;
        Ok(result.map(|v| StringValue::decode(&v).value))
    }

    pub async fn set(
        &self,
        key: Bytes,
        value: Bytes,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let key = StringKey::new(key);
        let value = StringValue::new(value);
        self.db.put(key.encode(), value.encode()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    async fn get_storage() -> (Storage, std::path::PathBuf) {
        let timestamp = ulid::Ulid::new().to_string();
        let path = std::env::temp_dir().join(format!("nimbis_test_{}", timestamp));
        std::fs::create_dir_all(&path).unwrap();
        let storage = Storage::open(&path).await.unwrap();
        (storage, path)
    }

    #[rstest]
    #[case("key1", "value1")]
    #[case("empty_val", "")]
    #[case("unicode_key_🔑", "unicode_val_🚀")]
    #[case("special_!@#", "value_!@#")]
    #[tokio::test]
    async fn test_storage_string_roundtrip(#[case] key: &str, #[case] value: &str) {
        let (storage, path) = get_storage().await;

        // Test set and get
        storage
            .set(Bytes::from(key.to_string()), Bytes::from(value.to_string()))
            .await
            .unwrap();
        let result = storage.get(Bytes::from(key.to_string())).await.unwrap();
        assert_eq!(result, Some(Bytes::copy_from_slice(value.as_bytes())));

        // Clean up
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn test_storage_string_missing() {
        let (storage, path) = get_storage().await;

        let missing = storage.get(Bytes::from("missing")).await.unwrap();
        assert_eq!(missing, None);

        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn test_storage_string_overwrite() {
        let (storage, path) = get_storage().await;

        storage
            .set(Bytes::from("key_overwrite"), Bytes::from("val1"))
            .await
            .unwrap();
        let result = storage.get(Bytes::from("key_overwrite")).await.unwrap();
        assert_eq!(result, Some(Bytes::from("val1")));

        storage
            .set(Bytes::from("key_overwrite"), Bytes::from("val2"))
            .await
            .unwrap();
        let result = storage.get(Bytes::from("key_overwrite")).await.unwrap();
        assert_eq!(result, Some(Bytes::from("val2")));

        let _ = std::fs::remove_dir_all(path);
    }
}
