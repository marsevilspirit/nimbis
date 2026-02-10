use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use slatedb::Db;
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

		// Open string_db first as it is needed by others for compaction filtering
		let string_db = open_db("/string", DataType::String, None).await?;
		let string_db = Arc::new(string_db);

		// Open collection DBs with reference to string_db
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

	#[rstest]
	#[tokio::test]
	async fn test_lazy_delete_zombie_isolation(#[future] ctx: TestContext) {
		let ctx = ctx.await;
		let key = Bytes::from("zombie_gen_test");

		// ZSET: Add member (Version 1)
		ctx.storage
			.zadd(key.clone(), vec![(1.0, Bytes::from("old_member"))])
			.await
			.unwrap();

		// Verify it's there
		let stored = ctx.storage.zrange(key.clone(), 0, -1, false).await.unwrap();
		assert_eq!(stored, vec![Bytes::from("old_member")]);

		// DEL (Logical Delete - only Meta)
		ctx.storage.del(key.clone()).await.unwrap();

		// Verify empty
		let exists = ctx.storage.exists(key.clone()).await.unwrap();
		assert!(!exists);

		// ZSET: Re-create (Version 2)
		ctx.storage
			.zadd(key.clone(), vec![(2.0, Bytes::from("new_member"))])
			.await
			.unwrap();

		// ONLY new member is visible
		// "old_member" is still in RocksDB but should be invisible due to version
		// mismatch
		let stored = ctx.storage.zrange(key.clone(), 0, -1, false).await.unwrap();
		assert_eq!(stored.len(), 1);
		assert_eq!(stored[0], Bytes::from("new_member"));
	}

	/// Verifies that after a logical delete (O(1)), the compaction filter
	/// correctly identifies all orphaned data for physical reclamation. This
	/// test detects potential "data leaks" where stale data remains on disk
	/// permanently.
	#[rstest]
	#[tokio::test]
	async fn test_physical_cleanup_after_logical_delete(#[future] ctx: TestContext) {
		use slatedb::CompactionFilter;

		use crate::compaction_filter::NimbisCompactionFilter;
		use crate::data_type::DataType;

		let ctx = ctx.await;
		let key = Bytes::from("leak_test_set");

		// SADD: Add multiple members
		let members: Vec<Bytes> = (0..10)
			.map(|i| Bytes::from(format!("member_{}", i)))
			.collect();
		let added = ctx
			.storage
			.sadd(key.clone(), members.clone())
			.await
			.unwrap();
		assert_eq!(added, 10);

		// Verify all members are logically visible
		let stored = ctx.storage.smembers(key.clone()).await.unwrap();
		assert_eq!(stored.len(), 10);

		// DEL: Logical delete (O(1) - only meta is removed)
		let deleted = ctx.storage.del(key.clone()).await.unwrap();
		assert!(deleted, "DEL should succeed");

		// Verify logically empty
		let exists = ctx.storage.exists(key.clone()).await.unwrap();
		assert!(!exists);

		// KEY VERIFICATION: Scan raw set_db to prove physical data still exists
		let scan_range = ..;
		let mut stream = ctx
			.storage
			.set_db
			.scan::<Bytes, _>(scan_range)
			.await
			.unwrap();
		let mut raw_count = 0;
		let mut raw_entries = Vec::new();
		while let Some(kv) = stream.next().await.unwrap() {
			raw_count += 1;
			raw_entries.push(kv);
		}
		// Physical data should still be present (zombie data)
		assert!(
			raw_count >= 10,
			"Expected at least 10 physical entries, found {}. Data was physically deleted instead of lazily!",
			raw_count
		);

		// Run compaction filter logic on all raw entries
		let mut filter = NimbisCompactionFilter {
			string_db: Some(ctx.storage.string_db.clone()),
			data_type: DataType::Set,
		};

		let mut reclaimed_count = 0;
		for kv in &raw_entries {
			let entry = slatedb::RowEntry {
				key: kv.key.clone(),
				value: slatedb::ValueDeletable::Value(kv.value.clone()),
				seq: 0,
				create_ts: None,
				expire_ts: None,
			};
			let decision = filter.filter(&entry).await.unwrap();
			if decision
				== slatedb::CompactionFilterDecision::Modify(slatedb::ValueDeletable::Tombstone)
			{
				reclaimed_count += 1;
			}
		}

		// ALL orphaned entries should be marked for reclamation
		assert_eq!(
			reclaimed_count,
			raw_count,
			"Data leak detected! {} out of {} entries were NOT reclaimed by the compaction filter",
			raw_count - reclaimed_count,
			raw_count
		);
	}
}
