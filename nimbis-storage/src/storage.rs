use std::sync::Arc;

use bytes::Bytes;
use nimbis_macros::storage_lock;
use slatedb::Db;
use slatedb::WriteBatch;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;
use slatedb::db_cache::foyer::FoyerCache;
use slatedb::object_store::ObjectStore;
use slatedb::object_store::ObjectStoreExt;
use slatedb::object_store::ObjectStoreScheme;
use slatedb::object_store::local::LocalFileSystem;
use slatedb::object_store::parse_url_opts;
use slatedb::object_store::path::Path as ObjectStorePath;
use tokio::sync::OnceCell;

use crate::compaction_filter::CollectionCompactionFilterSupplier;
use crate::data_type::DataType;
use crate::error::StorageError;
use crate::lock::StorageLock;
use crate::lock::StorageLockGuard;
use crate::lock::StorageLocks;
use crate::segment::NimbisSegmentExtractor;
use crate::segment::Segment;
use crate::string::meta::MetaKey;
use crate::string::meta::MetaValue;
use crate::utils::is_expired;
use crate::version::VersionGenerator;

#[derive(Clone)]
pub struct Storage {
	pub(crate) db: Arc<Db>,
	version_generator: Arc<VersionGenerator>,
	locks: Arc<StorageLocks>,
}

fn shard_path(base_path: ObjectStorePath, shard_id: Option<usize>) -> ObjectStorePath {
	match shard_id {
		Some(id) => base_path.join(format!("shard-{}", id)),
		None => base_path,
	}
}

fn local_path_url(path: &std::path::Path) -> Result<String, StorageError> {
	let abs_path = if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()?.join(path)
	};

	url::Url::from_file_path(&abs_path)
		.map(|url| url.to_string())
		.map_err(|_| StorageError::ObjectStoreConfig {
			message: format!(
				"failed to convert path '{}' to file URL",
				abs_path.display()
			),
		})
}

pub fn validate_object_store_url(url: &str) -> Result<(), StorageError> {
	let url = url::Url::parse(url)?;
	ObjectStoreScheme::parse(&url).map_err(|err| StorageError::ObjectStoreConfig {
		message: err.to_string(),
	})?;
	Ok(())
}

fn local_file_root(raw_url: &str, url: &url::Url) -> Result<std::path::PathBuf, StorageError> {
	let Some(path) = raw_url.strip_prefix("file:") else {
		return Ok(std::path::PathBuf::from(url.path()));
	};

	if path.is_empty() {
		Ok(std::path::PathBuf::from("."))
	} else if path.starts_with("//") {
		url.to_file_path()
			.map_err(|_| StorageError::ObjectStoreConfig {
				message: format!("invalid absolute file URL: {raw_url}"),
			})
	} else {
		Ok(std::path::PathBuf::from(path))
	}
}

async fn build_object_store<I, K, V>(
	raw_url: &str,
	url: &url::Url,
	options: I,
) -> Result<(Arc<dyn ObjectStore>, ObjectStorePath), StorageError>
where
	I: IntoIterator<Item = (K, V)>,
	K: AsRef<str>,
	V: Into<String>,
{
	let (scheme, _) =
		ObjectStoreScheme::parse(url).map_err(|err| StorageError::ObjectStoreConfig {
			message: err.to_string(),
		})?;

	if matches!(scheme, ObjectStoreScheme::Local) {
		let root = local_file_root(raw_url, url)?;
		tokio::fs::create_dir_all(&root).await?;
		let store = LocalFileSystem::new_with_prefix(root)?;
		return Ok((Arc::new(store), ObjectStorePath::from("")));
	}

	let (object_store, base_path) = parse_url_opts(url, options)?;
	Ok((Arc::from(object_store), base_path))
}

impl Storage {
	pub fn new(db: Arc<Db>) -> Self {
		Self {
			db,
			version_generator: Arc::new(VersionGenerator::new()),
			locks: Arc::new(StorageLocks::new()),
		}
	}

	pub(crate) fn next_generation(&self) -> u64 {
		self.version_generator.next()
	}

	pub(crate) async fn read_lock(
		&self,
		keys: impl IntoIterator<Item = Bytes>,
	) -> StorageLockGuard {
		let lock = StorageLock::read_keys(keys);
		self.locks.acquire(&lock).await
	}

	pub(crate) async fn write_lock(
		&self,
		keys: impl IntoIterator<Item = Bytes>,
	) -> StorageLockGuard {
		let lock = StorageLock::write_keys(keys);
		self.locks.acquire(&lock).await
	}

	pub(crate) async fn global_write_lock(&self) -> StorageLockGuard {
		let lock = StorageLock::global_write();
		self.locks.acquire(&lock).await
	}

	#[fastrace::trace]
	pub async fn open(
		path: impl AsRef<std::path::Path>,
		shard_id: Option<usize>,
	) -> Result<Self, StorageError> {
		let url = local_path_url(path.as_ref())?;
		Self::open_object_store(&url, std::iter::empty::<(&str, &str)>(), shard_id).await
	}

	#[fastrace::trace]
	pub async fn open_object_store<I, K, V>(
		url: &str,
		options: I,
		shard_id: Option<usize>,
	) -> Result<Self, StorageError>
	where
		I: IntoIterator<Item = (K, V)>,
		K: AsRef<str>,
		V: Into<String>,
	{
		let raw_url = url;
		let url = url::Url::parse(raw_url)?;
		let (object_store, base_path) = build_object_store(raw_url, &url, options).await?;
		let root_path = shard_path(base_path, shard_id);

		Self::open_with_object_store(object_store, root_path).await
	}

	async fn open_with_object_store(
		object_store: Arc<dyn ObjectStore>,
		root_path: ObjectStorePath,
	) -> Result<Self, StorageError> {
		let child_path = |name: &'static str| root_path.clone().join(name);

		let marker = child_path(".nimbis");
		object_store
			.put(&marker, bytes::Bytes::new().into())
			.await
			.map_err(StorageError::from)?;

		let cache = Arc::new(FoyerCache::new());
		let db_path = child_path("db");
		let filter_db = Arc::new(OnceCell::new());
		let compactor_builder =
			slatedb::CompactorBuilder::new(db_path.clone(), object_store.clone())
				.with_compaction_filter_supplier(Arc::new(CollectionCompactionFilterSupplier {
					db: filter_db.clone(),
				}));

		let db = Db::builder(db_path, object_store)
			.with_db_cache(cache)
			.with_segment_extractor(Arc::new(NimbisSegmentExtractor))
			.with_compactor_builder(compactor_builder)
			.build()
			.await
			.map_err(StorageError::from)?;
		let db = Arc::new(db);
		let _ = filter_db.set(db.clone());

		Ok(Self::new(db))
	}

	pub async fn close(&self) -> Result<(), StorageError> {
		self.db.close().await?;
		Ok(())
	}

	pub(crate) fn write_opts() -> WriteOptions {
		WriteOptions {
			await_durable: false,
			..WriteOptions::default()
		}
	}

	pub(crate) async fn write_batch(&self, batch: WriteBatch) -> Result<u64, StorageError> {
		let handle = self
			.db
			.write_with_options(batch, &Self::write_opts())
			.await?;
		Ok(handle.seqnum())
	}

	#[storage_lock(global_write)]
	#[fastrace::trace]
	pub async fn flush_all(&self) -> Result<(), StorageError> {
		let mut stream = self.db.scan::<Bytes, _>(..).await?;
		let mut batch = WriteBatch::new();
		let mut has_deletes = false;
		while let Some(kv) = stream.next().await? {
			batch.delete(kv.key);
			has_deletes = true;
		}

		if has_deletes {
			self.write_batch(batch).await?;
		}

		Ok(())
	}

	/// Helper to get and validate metadata for any collection type.
	/// Returns:
	/// - Ok(Some(meta)) if the key is a valid, non-expired meta of type T
	/// - Ok(None) if the key doesn't exist (expired keys are already filtered
	///   by storage)
	/// - Err if the key exists but is of wrong type
	pub(crate) async fn get_meta<T: MetaValue>(
		&self,
		key: &Bytes,
	) -> Result<Option<T>, StorageError> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = Segment::Meta.wrap(meta_key.encode());
		let kv = match self.db.get_key_value(meta_encoded_key.clone()).await? {
			Some(kv) => kv,
			None => return Ok(None),
		};

		if is_expired(kv.expire_ts) {
			let mut batch = WriteBatch::new();
			batch.delete(meta_encoded_key);
			self.write_batch(batch).await?;
			return Ok(None);
		}

		let meta_bytes = kv.value;

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

		let mut meta_val = T::decode(&meta_bytes)?;

		if let Some(ts) = kv.expire_ts {
			meta_val.set_expire_time(ts as u64);
		}

		Ok(Some(meta_val))
	}

	pub(crate) fn meta_put_opts(meta: &impl MetaValue) -> PutOptions {
		let ttl = meta
			.remaining_ttl()
			.map(|d| d.as_millis() as u64)
			.map(slatedb::config::Ttl::ExpireAfter)
			.unwrap_or(slatedb::config::Ttl::NoExpiry);
		PutOptions { ttl }
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
	async fn hset_writes_meta_and_field_in_one_segmented_batch(#[future] ctx: TestContext) {
		use crate::hash::field_key::HashFieldKey;
		use crate::segment::Segment;
		use crate::string::meta::HashMetaValue;

		let ctx = ctx.await;
		let key = Bytes::from("segmented_hash");
		let field = Bytes::from("field");
		let value = Bytes::from("value");

		let added = ctx
			.storage
			.hset(key.clone(), field.clone(), value.clone())
			.await
			.unwrap();
		assert_eq!(added, 1);

		let meta_key = Segment::Meta.wrap(MetaKey::new(key.clone()).encode());

		let meta_entry = ctx
			.storage
			.db
			.get_key_value(meta_key)
			.await
			.unwrap()
			.unwrap();
		let meta = HashMetaValue::decode(&meta_entry.value).unwrap();
		assert!(meta.version > 0);

		let field_key = Segment::Hash.wrap(HashFieldKey::new(key, meta.version, field).encode());
		let field_entry = ctx
			.storage
			.db
			.get_key_value(field_key)
			.await
			.unwrap()
			.unwrap();

		assert_eq!(field_entry.value, value);
	}

	#[rstest]
	#[tokio::test]
	async fn string_set_does_not_write_internal_seq_or_collection_keys(#[future] ctx: TestContext) {
		use crate::segment::Segment;

		let ctx = ctx.await;
		ctx.storage
			.set(Bytes::from("plain_string"), Bytes::from("value"))
			.await
			.unwrap();

		let mut stream = ctx.storage.db.scan::<Bytes, _>(..).await.unwrap();
		let mut seen = Vec::new();
		while let Some(kv) = stream.next().await.unwrap() {
			seen.push(kv.key.first().copied());
		}

		assert_eq!(seen, vec![Some(Segment::Meta.prefix())]);
	}

	#[rstest]
	#[tokio::test]
	async fn test_open_object_store_uses_url_path_and_shard_prefix() {
		let timestamp = ulid::Ulid::new().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_test_object_store_{}", timestamp));
		let url = local_path_url(path.as_path()).unwrap();

		let storage = Storage::open_object_store(&url, std::iter::empty::<(&str, &str)>(), Some(3))
			.await
			.unwrap();
		storage
			.set(Bytes::from("key"), Bytes::from("value"))
			.await
			.unwrap();
		storage.close().await.unwrap();

		assert!(path.join("shard-3").exists());
		let _ = std::fs::remove_dir_all(path);
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
		ctx.storage.del([key.clone()]).await.unwrap();

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

		use crate::compaction_filter::CollectionCompactionFilter;
		use crate::segment::Segment;
		use crate::utils::user_key_prefix;

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
		let deleted = ctx.storage.del([key.clone()]).await.unwrap();
		assert_eq!(deleted, 1, "DEL should delete one key");

		// Verify logically empty
		let exists = ctx.storage.exists(key.clone()).await.unwrap();
		assert!(!exists);

		// KEY VERIFICATION: Scan raw set segment to prove physical data still exists
		let prefix = Segment::Set.wrap(user_key_prefix(&key));
		let mut stream = ctx.storage.db.scan(prefix.clone()..).await.unwrap();
		let mut raw_count = 0;
		let mut raw_entries = Vec::new();
		while let Some(kv) = stream.next().await.unwrap() {
			if !kv.key.starts_with(&prefix) {
				break;
			}
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
		let filter_db = Arc::new(OnceCell::new());
		let _ = filter_db.set(ctx.storage.db.clone());
		let mut filter = CollectionCompactionFilter { db: filter_db };

		let mut reclaimed_count = 0;
		for kv in &raw_entries {
			let entry = slatedb::RowEntry {
				key: kv.key.clone(),
				value: slatedb::ValueDeletable::Value(kv.value.clone()),
				seq: kv.seq,
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

	#[test]
	fn test_meta_put_opts() {
		use slatedb::config::Ttl;

		use crate::string::meta::HashMetaValue;

		let mut val = HashMetaValue::new(1, 10);

		// Case 1: No expiration
		val.expire_time = 0;
		let opts = Storage::meta_put_opts(&val);
		assert_eq!(opts.ttl, Ttl::NoExpiry);

		// Case 2: Expired
		val.expire_time =
			(chrono::Utc::now().timestamp_millis().max(0) as u64).saturating_sub(1000);
		let opts = Storage::meta_put_opts(&val);
		assert_eq!(opts.ttl, Ttl::ExpireAfter(0));

		// Case 3: Future expiration
		let future = chrono::Utc::now().timestamp_millis().max(0) as u64 + 10000;
		val.expire_time = future;
		let opts = Storage::meta_put_opts(&val);
		if let Ttl::ExpireAfter(millis) = opts.ttl {
			assert!(millis > 0);
			assert!(millis <= 10000);
		} else {
			panic!("Expected ExpireAfter, got {:?}", opts.ttl);
		}
	}
}
