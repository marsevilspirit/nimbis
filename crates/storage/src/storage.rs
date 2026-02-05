use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use slatedb::Db;
use slatedb::WriteBatch;
use slatedb::config::WriteOptions;
use slatedb::db_cache::foyer::FoyerCache;
use slatedb::object_store::ObjectStore;
use slatedb::object_store::local::LocalFileSystem;

use crate::data_type::DataType;
use crate::error::StorageError;
use crate::string::meta::MetaKey;
use crate::string::meta::MetaValue;
use crate::version::VersionGenerator;

#[derive(Clone)]
pub struct Storage {
	pub(crate) string_db: Arc<Db>,
	pub(crate) hash_db: Arc<Db>,
	pub(crate) list_db: Arc<Db>,
	pub(crate) set_db: Arc<Db>,
	pub(crate) zset_db: Arc<Db>,
	pub(crate) version_generator: Arc<VersionGenerator>,
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
			version_generator: Arc::new(VersionGenerator::new()),
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

		// Create a single shared cache for all databases in this shard
		let cache = Arc::new(FoyerCache::new());

		let open_db = |name: &'static str, data_type: DataType, string_db: Option<Arc<Db>>| {
			let store = object_store.clone();
			let cache = cache.clone();
			async move {
				Db::builder(slatedb::object_store::path::Path::from(name), store)
					.with_memory_cache(cache)
					.with_compaction_filter_supplier(Arc::new(
						crate::compaction_filter::NimbisCompactionFilterSupplier {
							string_db,
							data_type,
						},
					))
					.build()
					.await
			}
		};

		// 1. Open string_db first as it is needed by others for compaction filtering
		let string_db = open_db("/string", DataType::String, None).await?;
		let string_db = Arc::new(string_db);

		// 2. Open collection DBs with reference to string_db
		let (hash_db, list_db, set_db, zset_db) = tokio::try_join!(
			open_db("/hash", DataType::Hash, Some(string_db.clone())),
			open_db("/list", DataType::List, Some(string_db.clone())),
			open_db("/set", DataType::Set, Some(string_db.clone())),
			open_db("/zset", DataType::ZSet, Some(string_db.clone()))
		)?;

		Ok(Self::new(
			string_db,
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
				expected: T::data_type(),
				actual: DataType::from_u8(actual_type_u8).unwrap_or(DataType::String),
			});
		}

		let meta_val = T::decode(&meta_bytes)?;
		if meta_val.is_expired() {
			#[cfg(not(test))]
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
		let range = prefix.clone()..;
		let mut stream = db.scan(range).await?;
		let mut batch = WriteBatch::new();
		let mut has_keys_to_delete = false;

		while let Some(kv) = stream.next().await? {
			if !kv.key.starts_with(&prefix) {
				break;
			}
			batch.delete(kv.key);
			has_keys_to_delete = true;
		}

		if has_keys_to_delete {
			let write_opts = WriteOptions {
				await_durable: false,
			};
			db.write_with_options(batch, &write_opts).await?;
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use rstest::*;

	use super::*;

	struct TestContext {
		storage: Storage,
		path: std::path::PathBuf,
	}

	impl Drop for TestContext {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.path);
		}
	}

	#[fixture]
	async fn ctx() -> TestContext {
		let timestamp = ulid::Ulid::new().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_test_storage_{}", timestamp));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		TestContext { storage, path }
	}

	async fn run_prefix_delete_test(
		ctx: &TestContext,
		prefix: &str,
		matching_keys: Vec<&str>,
		non_matching_keys: Vec<&str>,
	) {
		let prefix_bytes = Bytes::from(prefix.to_string());
		let value = Bytes::from("value");
		let write_opts = WriteOptions {
			await_durable: false,
		};

		for key in matching_keys.iter().chain(non_matching_keys.iter()) {
			ctx.storage
				.string_db
				.put_with_options(
					Bytes::from(key.to_string()),
					value.clone(),
					&Default::default(),
					&write_opts,
				)
				.await
				.unwrap();
		}

		ctx.storage
			.delete_keys_by_prefix(&ctx.storage.string_db, prefix_bytes)
			.await
			.unwrap();

		for key in &matching_keys {
			assert!(
				ctx.storage
					.string_db
					.get(Bytes::from(key.to_string()))
					.await
					.unwrap()
					.is_none()
			);
		}
		for key in &non_matching_keys {
			assert!(
				ctx.storage
					.string_db
					.get(Bytes::from(key.to_string()))
					.await
					.unwrap()
					.is_some()
			);
		}
	}

	#[rstest]
	#[case("test:", vec![], vec![])]
	#[case("test:", vec!["test:key1"], vec![])]
	#[case("test:", vec!["test:key1", "test:key2", "test:key3"], vec![])]
	#[case("test:", vec!["test:key1", "test:key2"], vec!["other:key1", "test", "testing:key"])]
	#[case("test:", vec![], vec!["other:key1", "another:key2"])]
	#[case("user:123:", vec!["user:123:name", "user:123:email"], vec!["user:456:name"])]
	#[case("用户:", vec!["用户:张三", "用户:李四"], vec!["管理员:王五"])]
	#[case("\x00\x01\x02:", vec!["\x00\x01\x02:key1"], vec!["normal:key"])]
	#[case("key:\n\t", vec!["key:\n\tvalue"], vec!["key:other"])]
	#[tokio::test]
	async fn test_delete_keys_by_prefix(
		#[future] ctx: TestContext,
		#[case] prefix: &str,
		#[case] matching_keys: Vec<&str>,
		#[case] non_matching_keys: Vec<&str>,
	) {
		run_prefix_delete_test(&ctx.await, prefix, matching_keys, non_matching_keys).await;
	}

	#[rstest]
	#[case("string_db", "hash_db")]
	#[case("list_db", "set_db")]
	#[case("zset_db", "string_db")]
	#[tokio::test]
	async fn test_delete_keys_by_prefix_different_databases(
		#[future] ctx: TestContext,
		#[case] db1_name: &str,
		#[case] db2_name: &str,
	) {
		let ctx = ctx.await;
		let get_db = |name: &str| match name {
			"string_db" => &ctx.storage.string_db,
			"hash_db" => &ctx.storage.hash_db,
			"list_db" => &ctx.storage.list_db,
			"set_db" => &ctx.storage.set_db,
			"zset_db" => &ctx.storage.zset_db,
			_ => panic!("Unknown database"),
		};
		let (db1, db2) = (get_db(db1_name), get_db(db2_name));
		let (prefix, key, value) = (
			Bytes::from("test:"),
			Bytes::from("test:key1"),
			Bytes::from("value"),
		);
		let write_opts = WriteOptions {
			await_durable: false,
		};

		db1.put_with_options(key.clone(), value.clone(), &Default::default(), &write_opts)
			.await
			.unwrap();
		db2.put_with_options(key.clone(), value, &Default::default(), &write_opts)
			.await
			.unwrap();

		ctx.storage
			.delete_keys_by_prefix(db1, prefix.clone())
			.await
			.unwrap();
		assert!(db1.get(key.clone()).await.unwrap().is_none());
		assert!(db2.get(key.clone()).await.unwrap().is_some());

		ctx.storage
			.delete_keys_by_prefix(db2, prefix)
			.await
			.unwrap();
		assert!(db2.get(key).await.unwrap().is_none());
	}
}
