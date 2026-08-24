use std::sync::Arc;

use bytes::Bytes;
use nimbis_macros::storage_lock;
use slatedb::Db;
#[cfg(test)]
use slatedb::config::PutOptions;
#[cfg(test)]
use slatedb::config::WriteOptions;
use slatedb::db_cache::foyer::FoyerCache;
use slatedb::object_store::ObjectStore;
use slatedb::object_store::ObjectStoreScheme;
use slatedb::object_store::local::LocalFileSystem;
use slatedb::object_store::parse_url_opts;
use slatedb::object_store::path::Path as ObjectStorePath;
#[cfg(test)]
use slatedb_common::metrics::DefaultMetricsRecorder;

use crate::compaction_filter::CollectionCompactionFilterSupplier;
use crate::data_type::DataType;
use crate::error::StorageError;
use crate::layout_migration::ensure_current_layout;
use crate::lock::StorageLock;
use crate::lock::StorageLockGuard;
use crate::lock::StorageLocks;
use crate::string::meta::HashMetaValue;
use crate::string::meta::ListMetaValue;
use crate::string::meta::SetMetaValue;
use crate::string::meta::ZSetMetaValue;
use crate::string::value::StringValue;
#[cfg(test)]
use crate::top_level_key::TopLevelKey;
use crate::typed_db::TypedDb;

#[derive(Clone)]
pub struct Storage {
	pub(crate) string_db: TypedDb<StringValue>,
	pub(crate) hash_db: TypedDb<HashMetaValue>,
	pub(crate) list_db: TypedDb<ListMetaValue>,
	pub(crate) set_db: TypedDb<SetMetaValue>,
	pub(crate) zset_db: TypedDb<ZSetMetaValue>,
	locks: Arc<StorageLocks>,
}

struct StorageDbs {
	string: OpenedDb,
	hash: OpenedDb,
	list: OpenedDb,
	set: OpenedDb,
	zset: OpenedDb,
}

struct OpenedDb {
	db: Arc<Db>,
	#[cfg(test)]
	metrics: Arc<DefaultMetricsRecorder>,
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
	fn from_dbs(dbs: StorageDbs) -> Self {
		Self {
			string_db: TypedDb::new(
				dbs.string.db,
				#[cfg(test)]
				dbs.string.metrics,
			),
			hash_db: TypedDb::new(
				dbs.hash.db,
				#[cfg(test)]
				dbs.hash.metrics,
			),
			list_db: TypedDb::new(
				dbs.list.db,
				#[cfg(test)]
				dbs.list.metrics,
			),
			set_db: TypedDb::new(
				dbs.set.db,
				#[cfg(test)]
				dbs.set.metrics,
			),
			zset_db: TypedDb::new(
				dbs.zset.db,
				#[cfg(test)]
				dbs.zset.metrics,
			),
			locks: Arc::new(StorageLocks::new()),
		}
	}

	pub(crate) async fn read_lock(
		&self,
		data_type: DataType,
		keys: impl IntoIterator<Item = Bytes>,
	) -> StorageLockGuard {
		let lock = StorageLock::read_keys(data_type, keys);
		self.locks.acquire(&lock).await
	}

	pub(crate) async fn write_lock(
		&self,
		data_type: DataType,
		keys: impl IntoIterator<Item = Bytes>,
	) -> StorageLockGuard {
		let lock = StorageLock::write_keys(data_type, keys);
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

		// Create a single shared cache for all databases in this shard
		let cache = Arc::new(FoyerCache::new());

		// Open string_db — no custom compaction filter needed;
		// SlateDB's built-in TTL mechanism handles expiration during compaction.
		let string_db = {
			let db_path = child_path("string");
			#[cfg(test)]
			let metrics = Arc::new(DefaultMetricsRecorder::new());
			let builder = Db::builder(db_path, object_store.clone()).with_db_cache(cache.clone());
			#[cfg(test)]
			let builder = builder.with_metrics_recorder(metrics.clone());
			let db = builder.build().await.map_err(StorageError::from)?;
			OpenedDb {
				db: Arc::new(db),
				#[cfg(test)]
				metrics,
			}
		};

		// Open every collection DB with a local, I/O-free compaction filter. Metadata
		// lives in the same DB as its sub-keys; the filter only uses metadata rows
		// that are already part of the ordered compaction stream.
		let open_db_with_collection_filter = |name: &'static str, data_type: DataType| {
			let store = object_store.clone();
			let cache = cache.clone();
			let db_path = child_path(name);
			async move {
				#[cfg(test)]
				let metrics = Arc::new(DefaultMetricsRecorder::new());
				let filter_supplier = Arc::new(CollectionCompactionFilterSupplier::new(data_type));
				let compactor_builder =
					slatedb::CompactorBuilder::new(db_path.clone(), store.clone())
						.with_compaction_filter_supplier(filter_supplier);
				let builder = Db::builder(db_path, store)
					.with_db_cache(cache)
					.with_compactor_builder(compactor_builder);
				#[cfg(test)]
				let builder = builder.with_metrics_recorder(metrics.clone());
				let db = builder.build().await.map_err(StorageError::from)?;
				Ok::<OpenedDb, StorageError>(OpenedDb {
					db: Arc::new(db),
					#[cfg(test)]
					metrics,
				})
			}
		};

		let (hash_db, list_db, set_db, zset_db) = tokio::try_join!(
			open_db_with_collection_filter("hash", DataType::Hash),
			open_db_with_collection_filter("list", DataType::List),
			open_db_with_collection_filter("set", DataType::Set),
			open_db_with_collection_filter("zset", DataType::ZSet)
		)?;

		let storage = Self::from_dbs(StorageDbs {
			string: string_db,
			hash: hash_db,
			list: list_db,
			set: set_db,
			zset: zset_db,
		});
		ensure_current_layout(&storage, object_store.as_ref(), &marker).await?;
		Ok(storage)
	}

	/// Return a physical database for lifecycle, migration, and corruption-test
	/// code. Command implementations should use their `TypedDb` field instead.
	pub(crate) fn raw_db_for_type(&self, data_type: DataType) -> &Db {
		match data_type {
			DataType::String => self.string_db.raw(),
			DataType::Hash => self.hash_db.raw(),
			DataType::List => self.list_db.raw(),
			DataType::Set => self.set_db.raw(),
			DataType::ZSet => self.zset_db.raw(),
		}
	}

	#[cfg(test)]
	pub(crate) fn metric_for_type(&self, data_type: DataType, name: &'static str) -> i64 {
		match data_type {
			DataType::String => self.string_db.metric(name),
			DataType::Hash => self.hash_db.metric(name),
			DataType::List => self.list_db.metric(name),
			DataType::Set => self.set_db.metric(name),
			DataType::ZSet => self.zset_db.metric(name),
		}
	}

	#[cfg(test)]
	pub(crate) fn all_raw_dbs(&self) -> [(DataType, &Db); 5] {
		[
			(DataType::String, self.string_db.raw()),
			(DataType::Hash, self.hash_db.raw()),
			(DataType::List, self.list_db.raw()),
			(DataType::Set, self.set_db.raw()),
			(DataType::ZSet, self.zset_db.raw()),
		]
	}

	pub(crate) fn collection_raw_dbs(&self) -> [(DataType, &Db); 4] {
		[
			(DataType::Hash, self.hash_db.raw()),
			(DataType::List, self.list_db.raw()),
			(DataType::Set, self.set_db.raw()),
			(DataType::ZSet, self.zset_db.raw()),
		]
	}

	pub async fn close(&self) -> Result<(), StorageError> {
		tokio::try_join!(
			self.hash_db.raw().close(),
			self.list_db.raw().close(),
			self.set_db.raw().close(),
			self.zset_db.raw().close(),
		)?;
		self.string_db.raw().close().await?;
		Ok(())
	}

	#[storage_lock(global_write)]
	#[fastrace::trace]
	pub async fn flush_all(&self) -> Result<(), StorageError> {
		// Iterate over all DBs and delete all keys
		// Since we don't have atomic flush_all, we do best effort sequential
		// Scanning and deleting everything is slow but correct for tests.
		// For production this is blocking and bad, but it's FLUSHDB.

		// Helper to clear a DB
		async fn clear_db(
			db: &slatedb::Db,
		) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
			let mut stream = db.scan(..).await?;
			let write_opts = slatedb::config::WriteOptions::default();
			while let Some(kv) = stream.next().await? {
				db.delete_with_options(kv.key, &write_opts).await?;
			}
			Ok(())
		}

		clear_db(self.string_db.raw()).await?;
		clear_db(self.hash_db.raw()).await?;
		clear_db(self.list_db.raw()).await?;
		clear_db(self.set_db.raw()).await?;
		clear_db(self.zset_db.raw()).await?;

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use rstest::*;

	use super::*;
	use crate::layout_migration::test_support::CURRENT_LAYOUT_VERSION;
	use crate::layout_migration::test_support::MAX_MIGRATION_TTL_DRIFT_MS;
	use crate::layout_migration::test_support::MIGRATION_SCAN_CHUNK_SIZE;
	use crate::layout_migration::test_support::logical_expire_ts;

	struct TestContext {
		storage: Storage,
		path: std::path::PathBuf,
	}

	impl Drop for TestContext {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.path);
		}
	}

	async fn mark_layout_legacy(path: &std::path::Path) {
		tokio::fs::write(path.join(".nimbis"), Bytes::new())
			.await
			.unwrap();
	}

	async fn assert_current_layout_marker(path: &std::path::Path) {
		assert_eq!(
			tokio::fs::read(path.join(".nimbis")).await.unwrap(),
			CURRENT_LAYOUT_VERSION
		);
	}

	#[fixture]
	async fn ctx() -> TestContext {
		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_test_storage_{}", timestamp));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		TestContext { storage, path }
	}

	#[rstest]
	#[tokio::test]
	async fn test_open_object_store_uses_url_path_and_shard_prefix() {
		let timestamp = ulid::Ulid::generate().to_string();
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
		assert_current_layout_marker(&path.join("shard-3")).await;
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
		ctx.storage
			.del(DataType::ZSet, [key.clone()])
			.await
			.unwrap();

		// Verify empty
		let exists = ctx
			.storage
			.exists(DataType::ZSet, key.clone())
			.await
			.unwrap();
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

	#[tokio::test]
	async fn test_startup_migrates_every_legacy_collection_metadata_type() {
		use crate::string::meta::CollectionMeta;
		use crate::string::meta::HashMetaValue;
		use crate::string::meta::ListMetaValue;
		use crate::string::meta::SetMetaValue;
		use crate::string::meta::ZSetMetaValue;
		use crate::typed_db::metadata_put_options;

		async fn move_meta_to_legacy<T: CollectionMeta>(
			source: &Db,
			string_db: &Db,
			key: &Bytes,
			delete_source: bool,
		) -> u64 {
			let encoded_key = TopLevelKey::new(key.clone()).unwrap().encode();
			let source_row = source
				.get_key_value(encoded_key.clone())
				.await
				.unwrap()
				.unwrap();
			let mut meta = T::decode(&source_row.value).unwrap();
			meta.resolve_pending_generation(source_row.seq);
			let write_opts = WriteOptions::default();
			let put_opts = metadata_put_options(&meta).unwrap();
			string_db
				.put_with_options(encoded_key.clone(), meta.encode(), &put_opts, &write_opts)
				.await
				.unwrap();
			if delete_source {
				source
					.delete_with_options(encoded_key, &write_opts)
					.await
					.unwrap();
			}
			meta.expire_time()
		}

		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_layout_migration_{timestamp}"));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		let hash_key = Bytes::from_static(b"legacy-hash");
		let list_key = Bytes::from_static(b"legacy-list");
		let set_key = Bytes::from_static(b"legacy-set");
		let zset_key = Bytes::from_static(b"legacy-zset");

		storage
			.hset(
				hash_key.clone(),
				Bytes::from_static(b"field"),
				Bytes::from_static(b"value"),
			)
			.await
			.unwrap();
		storage
			.rpush(list_key.clone(), vec![Bytes::from_static(b"element")])
			.await
			.unwrap();
		storage
			.sadd(set_key.clone(), vec![Bytes::from_static(b"member")])
			.await
			.unwrap();
		storage
			.zadd(zset_key.clone(), vec![(1.0, Bytes::from_static(b"member"))])
			.await
			.unwrap();

		let expire_time = (chrono::Utc::now().timestamp_millis().max(0) as u64) + 120_000;
		for (data_type, key) in [
			(DataType::Hash, &hash_key),
			(DataType::List, &list_key),
			(DataType::Set, &set_key),
			(DataType::ZSet, &zset_key),
		] {
			assert!(
				storage
					.expire(data_type, key.clone(), expire_time)
					.await
					.unwrap()
			);
		}

		// Hash deliberately remains in both locations. This is the durable-target /
		// source-not-deleted state left by a crash between the two migration writes.
		let migrated_expirations = [
			move_meta_to_legacy::<HashMetaValue>(
				storage.hash_db.raw(),
				storage.string_db.raw(),
				&hash_key,
				false,
			)
			.await,
			move_meta_to_legacy::<ListMetaValue>(
				storage.list_db.raw(),
				storage.string_db.raw(),
				&list_key,
				true,
			)
			.await,
			move_meta_to_legacy::<SetMetaValue>(
				storage.set_db.raw(),
				storage.string_db.raw(),
				&set_key,
				true,
			)
			.await,
			move_meta_to_legacy::<ZSetMetaValue>(
				storage.zset_db.raw(),
				storage.string_db.raw(),
				&zset_key,
				true,
			)
			.await,
		];
		assert_eq!(migrated_expirations, [expire_time; 4]);
		storage.close().await.unwrap();
		drop(storage);
		mark_layout_legacy(&path).await;

		let storage = Storage::open(&path, None).await.unwrap();
		assert_current_layout_marker(&path).await;
		assert_eq!(
			storage
				.hget(hash_key.clone(), Bytes::from_static(b"field"))
				.await
				.unwrap(),
			Some(Bytes::from_static(b"value"))
		);
		assert_eq!(
			storage.lrange(list_key.clone(), 0, -1).await.unwrap(),
			vec![Bytes::from_static(b"element")]
		);
		assert!(
			storage
				.sismember(set_key.clone(), Bytes::from_static(b"member"))
				.await
				.unwrap()
		);
		assert_eq!(
			storage
				.zscore(zset_key.clone(), Bytes::from_static(b"member"))
				.await
				.unwrap(),
			Some(1.0)
		);

		for (data_type, key) in [
			(DataType::Hash, hash_key),
			(DataType::List, list_key),
			(DataType::Set, set_key),
			(DataType::ZSet, zset_key),
		] {
			let encoded_key = TopLevelKey::new(key).unwrap().encode();
			assert!(
				storage
					.string_db
					.raw()
					.get(encoded_key.clone())
					.await
					.unwrap()
					.is_none()
			);
			let migrated = storage
				.raw_db_for_type(data_type)
				.get_key_value(encoded_key)
				.await
				.unwrap()
				.unwrap();
			assert_eq!(
				logical_expire_ts(&migrated).unwrap(),
				Some(expire_time as i64)
			);
			let row_expire_ts = migrated.expire_ts.unwrap();
			assert!(row_expire_ts >= expire_time as i64);
			assert!(row_expire_ts - expire_time as i64 <= MAX_MIGRATION_TTL_DRIFT_MS);
		}
		storage.close().await.unwrap();
		drop(storage);

		// A second startup is idempotent and does not need a legacy read path.
		let storage = Storage::open(&path, None).await.unwrap();
		assert_eq!(
			storage
				.hlen(Bytes::from_static(b"legacy-hash"))
				.await
				.unwrap(),
			1
		);
		storage.close().await.unwrap();
		drop(storage);
		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_startup_rejects_malformed_legacy_collection_metadata_without_publishing_marker() {
		use bytes::BufMut;
		use bytes::BytesMut;

		use crate::string::meta::HashMetaValue;

		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_malformed_legacy_key_{timestamp}"));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();

		// SlateDB itself rejects encoded keys longer than u16::MAX, so a historical
		// 65,536-byte user key could never have been persisted through its write API.
		// This is the largest persistable row and exercises the same unsafe case: a
		// collection metadata value whose top-level key boundary is not exact.
		let key = vec![b'k'; usize::from(u16::MAX) - 2];
		let mut encoded_key = BytesMut::with_capacity(2 + key.len());
		encoded_key.put_u16(0);
		encoded_key.extend_from_slice(&key);
		let encoded_key = encoded_key.freeze();
		assert_eq!(&encoded_key[..2], &[0, 0]);
		storage
			.string_db
			.raw()
			.put(encoded_key, HashMetaValue::new(1, 1).encode())
			.await
			.unwrap();
		storage.close().await.unwrap();
		drop(storage);
		mark_layout_legacy(&path).await;

		let error = match Storage::open(&path, None).await {
			Ok(storage) => {
				storage.close().await.unwrap();
				panic!("malformed legacy key must not publish the current layout")
			}
			Err(error) => error,
		};
		assert!(matches!(
			error,
			StorageError::DataInconsistency { message }
				if message.contains("invalid top-level key length")
		));
		assert_eq!(tokio::fs::read(path.join(".nimbis")).await.unwrap(), b"");

		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_startup_recovers_string_from_hash_only_candidate_layout() {
		use crate::string::value::StringValue;

		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_string_recovery_{timestamp}"));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		let key = Bytes::from_static(b"misplaced-string");
		let encoded_key = TopLevelKey::new(key.clone()).unwrap().encode();
		storage
			.hash_db
			.raw()
			.put(encoded_key.clone(), StringValue::new("value").encode())
			.await
			.unwrap();
		storage.close().await.unwrap();
		drop(storage);
		mark_layout_legacy(&path).await;

		let storage = Storage::open(&path, None).await.unwrap();
		assert_eq!(
			storage.get(key).await.unwrap(),
			Some(Bytes::from_static(b"value"))
		);
		assert!(
			storage
				.hash_db
				.raw()
				.get(encoded_key.clone())
				.await
				.unwrap()
				.is_none()
		);
		assert!(
			storage
				.string_db
				.raw()
				.get(encoded_key)
				.await
				.unwrap()
				.is_some()
		);
		storage.close().await.unwrap();
		drop(storage);
		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_string_migration_reuses_non_shortening_ttl_destination() {
		use slatedb::config::Ttl;

		use crate::string::value::StringValue;

		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_string_retry_{timestamp}"));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		let key = Bytes::from_static(b"misplaced-string-retry");
		let encoded_key = TopLevelKey::new(key.clone()).unwrap().encode();
		let encoded_value = StringValue::new("value").encode();
		let put_opts = PutOptions {
			ttl: Ttl::ExpireAfterMillis(120_000),
		};
		let write_opts = WriteOptions::default();

		storage
			.hash_db
			.raw()
			.put_with_options(
				encoded_key.clone(),
				encoded_value.clone(),
				&put_opts,
				&write_opts,
			)
			.await
			.unwrap();
		let source_expire_ts = storage
			.hash_db
			.raw()
			.get_key_value(encoded_key.clone())
			.await
			.unwrap()
			.unwrap()
			.expire_ts
			.unwrap();
		storage
			.string_db
			.raw()
			.put_with_options(encoded_key.clone(), encoded_value, &put_opts, &write_opts)
			.await
			.unwrap();
		let destination_expire_ts = storage
			.string_db
			.raw()
			.get_key_value(encoded_key.clone())
			.await
			.unwrap()
			.unwrap()
			.expire_ts
			.unwrap();
		assert!(destination_expire_ts >= source_expire_ts);
		assert!(destination_expire_ts - source_expire_ts <= MAX_MIGRATION_TTL_DRIFT_MS);
		storage.close().await.unwrap();
		drop(storage);
		mark_layout_legacy(&path).await;

		let storage = Storage::open(&path, None).await.unwrap();
		assert_eq!(
			storage.get(key).await.unwrap(),
			Some(Bytes::from_static(b"value"))
		);
		assert!(
			storage
				.hash_db
				.raw()
				.get(encoded_key.clone())
				.await
				.unwrap()
				.is_none()
		);
		assert_eq!(
			storage
				.string_db
				.raw()
				.get_key_value(encoded_key)
				.await
				.unwrap()
				.unwrap()
				.expire_ts,
			Some(destination_expire_ts)
		);
		assert_current_layout_marker(&path).await;
		storage.close().await.unwrap();
		drop(storage);
		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_current_layout_marker_skips_migration_on_second_open() {
		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_marker_skip_{timestamp}"));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		assert_current_layout_marker(&path).await;

		// This exact top-level key looks like legacy Hash metadata by its type byte,
		// but is intentionally undecodable. A migration scan would make the next
		// open fail; the current marker must bypass it.
		let encoded_key = TopLevelKey::new("skip-migration-sentinel")
			.unwrap()
			.encode();
		let invalid_hash_meta = Bytes::from(vec![DataType::Hash as u8]);
		storage
			.string_db
			.raw()
			.put(encoded_key.clone(), invalid_hash_meta.clone())
			.await
			.unwrap();
		storage.close().await.unwrap();
		drop(storage);

		let storage = Storage::open(&path, None).await.unwrap();
		assert_eq!(
			storage.string_db.raw().get(encoded_key).await.unwrap(),
			Some(invalid_hash_meta)
		);
		storage.close().await.unwrap();
		drop(storage);
		let _ = std::fs::remove_dir_all(path);
	}

	#[tokio::test]
	async fn test_startup_migration_crosses_scan_chunk_boundary() {
		use crate::string::value::StringValue;

		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_chunked_migration_{timestamp}"));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		let count = MIGRATION_SCAN_CHUNK_SIZE + 1;
		let put_opts = PutOptions::default();
		let write_opts = WriteOptions::default();
		for index in 0..count {
			let key = Bytes::from(format!("chunked-string-{index:04}"));
			storage
				.hash_db
				.raw()
				.put_with_options(
					TopLevelKey::new(key).unwrap().encode(),
					StringValue::new(format!("value-{index}")).encode(),
					&put_opts,
					&write_opts,
				)
				.await
				.unwrap();
		}
		storage.close().await.unwrap();
		drop(storage);
		mark_layout_legacy(&path).await;

		let storage = Storage::open(&path, None).await.unwrap();
		for index in 0..count {
			let key = Bytes::from(format!("chunked-string-{index:04}"));
			let encoded_key = TopLevelKey::new(key.clone()).unwrap().encode();
			assert_eq!(
				storage.get(key).await.unwrap(),
				Some(Bytes::from(format!("value-{index}")))
			);
			assert!(
				storage
					.hash_db
					.raw()
					.get(encoded_key)
					.await
					.unwrap()
					.is_none()
			);
		}
		storage.close().await.unwrap();
		drop(storage);
		let _ = std::fs::remove_dir_all(path);
	}

	#[test]
	fn test_metadata_put_options() {
		use slatedb::config::Ttl;

		use crate::string::meta::HashMetaValue;

		let mut val = HashMetaValue::new(1, 10);

		// Case 1: No expiration
		val.expire_time = 0;
		let opts = crate::typed_db::metadata_put_options(&val).unwrap();
		assert_eq!(opts.ttl, Ttl::NoExpiry);

		// Case 2: Expired
		val.expire_time =
			(chrono::Utc::now().timestamp_millis().max(0) as u64).saturating_sub(1000);
		let opts = crate::typed_db::metadata_put_options(&val).unwrap();
		assert_eq!(opts.ttl, Ttl::ExpireAfterMillis(0));

		// Case 3: Future expiration
		let future = chrono::Utc::now().timestamp_millis().max(0) as u64 + 10000;
		val.expire_time = future;
		let opts = crate::typed_db::metadata_put_options(&val).unwrap();
		if let Ttl::ExpireAfterMillis(millis) = opts.ttl {
			assert!(millis > 0);
			assert!(millis <= 10000);
		} else {
			panic!("Expected ExpireAfterMillis, got {:?}", opts.ttl);
		}
	}
}
