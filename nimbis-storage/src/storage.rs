use std::ops::Bound;
use std::sync::Arc;

use bytes::Buf;
use bytes::Bytes;
use nimbis_macros::storage_lock;
use slatedb::Db;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;
use slatedb::db_cache::foyer::FoyerCache;
use slatedb::object_store::ObjectStore;
use slatedb::object_store::ObjectStoreScheme;
use slatedb::object_store::local::LocalFileSystem;
use slatedb::object_store::parse_url_opts;
use slatedb::object_store::path::Path as ObjectStorePath;

use crate::compaction_filter::CollectionCompactionFilterSupplier;
use crate::data_type::DataType;
use crate::error::StorageError;
use crate::lock::StorageLock;
use crate::lock::StorageLockGuard;
use crate::lock::StorageLocks;
use crate::string::meta::AnyValue;
use crate::string::meta::MetaKey;
use crate::string::meta::MetaValue;
use crate::utils::is_expired;

const CURRENT_LAYOUT_VERSION: &[u8] = b"nimbis-layout:type-local-metadata:v1\n";
const MIGRATION_SCAN_CHUNK_SIZE: usize = 64;
const MAX_MIGRATION_TTL_DRIFT_MS: i64 = 5_000;

struct NormalizedTopLevel {
	value: AnyValue,
	logical_expire_ts: Option<i64>,
	row_expire_ts: Option<i64>,
}

#[derive(Clone)]
pub struct Storage {
	pub(crate) string_db: Arc<Db>,
	pub(crate) hash_db: Arc<Db>,
	pub(crate) list_db: Arc<Db>,
	pub(crate) set_db: Arc<Db>,
	pub(crate) zset_db: Arc<Db>,
	locks: Arc<StorageLocks>,
}

fn shard_path(base_path: ObjectStorePath, shard_id: Option<usize>) -> ObjectStorePath {
	match shard_id {
		Some(id) => base_path.child(format!("shard-{}", id)),
		None => base_path,
	}
}

fn decode_exact_meta_key(encoded: &[u8]) -> Option<Bytes> {
	if encoded.len() < 2 {
		return None;
	}
	let mut remaining = encoded;
	let key_len = remaining.get_u16() as usize;
	if remaining.len() != key_len {
		return None;
	}
	Some(Bytes::copy_from_slice(remaining))
}

async fn layout_marker_is_current(
	object_store: &dyn ObjectStore,
	marker: &ObjectStorePath,
) -> Result<bool, StorageError> {
	match object_store.get(marker).await {
		Ok(result) => Ok(result.bytes().await?.as_ref() == CURRENT_LAYOUT_VERSION),
		Err(slatedb::object_store::Error::NotFound { .. }) => Ok(false),
		Err(error) => Err(error.into()),
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
			locks: Arc::new(StorageLocks::new()),
		}
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
		let child_path = |name: &'static str| root_path.child(name);

		let marker = child_path(".nimbis");
		let layout_is_current = layout_marker_is_current(object_store.as_ref(), &marker).await?;

		// Create a single shared cache for all databases in this shard
		let cache = Arc::new(FoyerCache::new());

		// Open string_db — no custom compaction filter needed;
		// SlateDB's built-in TTL mechanism handles expiration during compaction.
		let string_db = {
			let db_path = child_path("string");
			let db = Db::builder(db_path, object_store.clone())
				.with_db_cache(cache.clone())
				.build()
				.await
				.map_err(StorageError::from)?;
			Arc::new(db)
		};

		// Open every collection DB with a local, I/O-free compaction filter. Metadata
		// lives in the same DB as its sub-keys; the filter only uses metadata rows
		// that are already part of the ordered compaction stream.
		let open_db_with_collection_filter = |name: &'static str, data_type: DataType| {
			let store = object_store.clone();
			let cache = cache.clone();
			let db_path = child_path(name);
			async move {
				let filter_supplier = Arc::new(CollectionCompactionFilterSupplier::new(data_type));
				let compactor_builder =
					slatedb::CompactorBuilder::new(db_path.clone(), store.clone())
						.with_compaction_filter_supplier(filter_supplier);
				let db = Db::builder(db_path, store)
					.with_db_cache(cache)
					.with_compactor_builder(compactor_builder)
					.build()
					.await
					.map_err(StorageError::from)?;
				Ok::<Arc<Db>, StorageError>(Arc::new(db))
			}
		};

		let (hash_db, list_db, set_db, zset_db) = tokio::try_join!(
			open_db_with_collection_filter("hash", DataType::Hash),
			open_db_with_collection_filter("list", DataType::List),
			open_db_with_collection_filter("set", DataType::Set),
			open_db_with_collection_filter("zset", DataType::ZSet)
		)?;

		let storage = Self::new(string_db, hash_db, list_db, set_db, zset_db);
		// This runs before the server can accept commands. It is intentionally
		// durable and idempotent so databases created by the old split-metadata
		// layout converge to a single local authority without a runtime dual-read
		// fallback.
		if !layout_is_current {
			storage.migrate_legacy_layout().await?;
			// Publishing the version is the migration commit point. Every destination
			// write and source delete is durable before this marker is replaced, so an
			// empty/old marker always remains safe to retry after interruption.
			object_store
				.put(&marker, Bytes::from_static(CURRENT_LAYOUT_VERSION).into())
				.await
				.map_err(StorageError::from)?;
		}
		Ok(storage)
	}

	pub(crate) fn db_for_type(&self, data_type: DataType) -> &Db {
		match data_type {
			DataType::String => &self.string_db,
			DataType::Hash => &self.hash_db,
			DataType::List => &self.list_db,
			DataType::Set => &self.set_db,
			DataType::ZSet => &self.zset_db,
		}
	}

	pub(crate) fn typed_dbs(&self) -> [(DataType, &Db); 5] {
		[
			(DataType::String, &self.string_db),
			(DataType::Hash, &self.hash_db),
			(DataType::List, &self.list_db),
			(DataType::Set, &self.set_db),
			(DataType::ZSet, &self.zset_db),
		]
	}

	fn normalized_top_level(kv: &slatedb::KeyValue) -> Result<NormalizedTopLevel, StorageError> {
		let mut value = AnyValue::decode(&kv.value)?;
		if value.version() == Some(0) {
			value.set_version(kv.seq);
		}

		let logical_expire_ts = if value.data_type() == DataType::String {
			// StringValue has no embedded expiration field. Its SlateDB row TTL is
			// therefore the only recoverable logical deadline during migration.
			kv.expire_ts
		} else {
			let encoded_expire_time = value.expire_time();
			if encoded_expire_time == 0 {
				// Older metadata may have relied only on the row TTL. Canonicalize that
				// deadline into the payload before it is moved to the typed database.
				if let Some(expire_ts) = kv.expire_ts {
					value.set_expire_time(expire_ts.max(0) as u64);
				}
				kv.expire_ts
			} else {
				Some(i64::try_from(encoded_expire_time).map_err(|_| {
					StorageError::DataInconsistency {
						message: "metadata expiration exceeds SlateDB timestamp range".to_string(),
					}
				})?)
			}
		};

		Ok(NormalizedTopLevel {
			value,
			logical_expire_ts,
			row_expire_ts: kv.expire_ts,
		})
	}

	fn destination_matches_source(
		source: &NormalizedTopLevel,
		destination: &NormalizedTopLevel,
		expected_type: DataType,
	) -> bool {
		if destination.value.data_type() != expected_type
			|| destination.value.encode() != source.value.encode()
		{
			return false;
		}

		match expected_type {
			DataType::String => match (source.logical_expire_ts, destination.row_expire_ts) {
				(None, None) => true,
				// ExpireAfter is resolved by the destination DB after the migration
				// call is queued. Accept only a non-shortening deadline: this makes a
				// durable destination write safe to reuse after a crash without risking
				// premature String loss.
				(Some(source_expire_ts), Some(destination_expire_ts)) => {
					destination_expire_ts >= source_expire_ts
						&& destination_expire_ts.saturating_sub(source_expire_ts)
							<= MAX_MIGRATION_TTL_DRIFT_MS
				}
				_ => false,
			},
			_ => {
				if destination.logical_expire_ts != source.logical_expire_ts {
					return false;
				}
				match (source.logical_expire_ts, destination.row_expire_ts) {
					(None, None) => true,
					(Some(logical_expire_ts), Some(row_expire_ts)) => {
						row_expire_ts >= logical_expire_ts
							&& row_expire_ts.saturating_sub(logical_expire_ts)
								<= MAX_MIGRATION_TTL_DRIFT_MS
					}
					_ => false,
				}
			}
		}
	}

	fn migration_put_opts(logical_expire_ts: Option<i64>) -> PutOptions {
		let ttl = logical_expire_ts
			.map(|expire_ts| {
				let now = chrono::Utc::now().timestamp_millis();
				slatedb::config::Ttl::ExpireAfter(expire_ts.saturating_sub(now).max(0) as u64)
			})
			.unwrap_or(slatedb::config::Ttl::NoExpiry);
		PutOptions { ttl }
	}

	async fn copy_top_level_durably(
		&self,
		source_db: &Db,
		destination_db: &Db,
		encoded_key: Bytes,
		source_kv: slatedb::KeyValue,
		expected_type: DataType,
	) -> Result<(), StorageError> {
		let durable = WriteOptions {
			await_durable: true,
		};
		if is_expired(source_kv.expire_ts) {
			source_db.delete_with_options(encoded_key, &durable).await?;
			return Ok(());
		}

		let source_value = Self::normalized_top_level(&source_kv)?;
		if source_value.value.data_type() != expected_type {
			return Err(StorageError::DataInconsistency {
				message: format!(
					"layout migration expected {expected_type:?}, found {:?}",
					source_value.value.data_type()
				),
			});
		}
		if is_expired(source_value.logical_expire_ts) {
			source_db.delete_with_options(encoded_key, &durable).await?;
			return Ok(());
		}
		let normalized_source = source_value.value.encode();

		if let Some(destination_kv) = destination_db.get_key_value(encoded_key.clone()).await? {
			let destination_value = Self::normalized_top_level(&destination_kv)?;
			if !Self::destination_matches_source(&source_value, &destination_value, expected_type) {
				return Err(StorageError::DataInconsistency {
					message: format!(
						"conflicting {expected_type:?} metadata authorities during layout migration"
					),
				});
			}
		} else {
			let put_opts = Self::migration_put_opts(source_value.logical_expire_ts);
			destination_db
				.put_with_options(
					encoded_key.clone(),
					normalized_source.clone(),
					&put_opts,
					&durable,
				)
				.await?;

			let Some(destination_kv) = destination_db.get_key_value(encoded_key.clone()).await?
			else {
				// A key can expire while it is being migrated. The source remains a
				// valid recovery authority unless it is now expired as well.
				if is_expired(source_value.logical_expire_ts) {
					source_db.delete_with_options(encoded_key, &durable).await?;
					return Ok(());
				}
				return Err(StorageError::DataInconsistency {
					message: format!(
						"{expected_type:?} metadata was not visible after durable migration write"
					),
				});
			};
			let destination_value = Self::normalized_top_level(&destination_kv)?;
			if !Self::destination_matches_source(&source_value, &destination_value, expected_type) {
				// This destination did not exist before this invocation. Remove an
				// incompatible copy so an old marker can retry instead of becoming
				// permanently wedged on the next startup.
				destination_db
					.delete_with_options(encoded_key.clone(), &durable)
					.await?;
				return Err(StorageError::DataInconsistency {
					message: format!(
						"{expected_type:?} metadata verification failed after migration"
					),
				});
			}
		}

		// The destination write is durable before the legacy authority is removed.
		// A crash before this delete leaves two compatible copies; the next startup
		// verifies payload, logical expiration and bounded row-TTL drift before it
		// completes the delete.
		source_db.delete_with_options(encoded_key, &durable).await?;
		Ok(())
	}

	async fn migrate_legacy_source(
		&self,
		source_type: DataType,
		source_db: &Db,
	) -> Result<(), StorageError> {
		let mut cursor = None;
		loop {
			let start = cursor.map_or(Bound::Unbounded, Bound::Excluded);
			let mut stream = source_db
				.scan::<Bytes, _>((start, Bound::Unbounded))
				.await?;
			let mut candidates = Vec::with_capacity(MIGRATION_SCAN_CHUNK_SIZE);
			let mut scanned = 0;
			let mut last_scanned_key = None;

			while scanned < MIGRATION_SCAN_CHUNK_SIZE {
				let Some(kv) = stream.next().await? else {
					break;
				};
				scanned += 1;
				last_scanned_key = Some(kv.key.clone());
				if decode_exact_meta_key(&kv.key).is_none() || kv.value.is_empty() {
					continue;
				}
				let Some(encoded_type) = DataType::from_u8(kv.value[0]) else {
					continue;
				};
				let is_legacy = match source_type {
					DataType::String => encoded_type != DataType::String,
					_ => encoded_type == DataType::String,
				};
				if is_legacy {
					candidates.push((encoded_type, kv));
				}
			}
			drop(stream);

			for (destination_type, kv) in candidates {
				self.copy_top_level_durably(
					source_db,
					self.db_for_type(destination_type),
					kv.key.clone(),
					kv,
					destination_type,
				)
				.await?;
			}

			if scanned < MIGRATION_SCAN_CHUNK_SIZE {
				break;
			}
			cursor = last_scanned_key;
		}
		Ok(())
	}

	async fn migrate_legacy_layout(&self) -> Result<(), StorageError> {
		// First recover Strings written into hash_db by the earlier hash-only
		// co-location candidate. New writes never place Strings in collection DBs.
		for (data_type, db) in self.typed_dbs().into_iter().skip(1) {
			self.migrate_legacy_source(data_type, db).await?;
		}

		// Then move legacy collection metadata out of string_db. Each scan is
		// bounded and dropped before its source rows are durably deleted.
		self.migrate_legacy_source(DataType::String, &self.string_db)
			.await?;
		Ok(())
	}

	pub async fn close(&self) -> Result<(), StorageError> {
		tokio::try_join!(
			self.hash_db.close(),
			self.list_db.close(),
			self.set_db.close(),
			self.zset_db.close(),
		)?;
		self.string_db.close().await?;
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

	/// Get and validate top-level data from its authoritative typed database.
	/// Returns:
	/// - Ok(Some(meta)) if the key is a valid, non-expired meta of type T
	/// - Ok(None) if the key doesn't exist (expired keys are already filtered
	///   by storage)
	/// - Err if the key exists but is of wrong type
	pub(crate) async fn get_meta<T: MetaValue>(
		&self,
		key: &Bytes,
	) -> Result<Option<T>, StorageError> {
		let db = T::data_type()
			.map(|data_type| self.db_for_type(data_type))
			// AnyValue is used for String decoding only; cross-type inspection is
			// explicit through typed_dbs so same-name values can coexist.
			.unwrap_or(&self.string_db);
		Self::get_meta_from_db::<T>(db, key).await
	}

	pub(crate) async fn get_meta_from_db<T: MetaValue>(
		db: &Db,
		key: &Bytes,
	) -> Result<Option<T>, StorageError> {
		let meta_key = MetaKey::new(key.clone());
		let meta_encoded_key = meta_key.encode();
		let kv = match db.get_key_value(meta_encoded_key.clone()).await? {
			Some(kv) => kv,
			None => return Ok(None),
		};

		if is_expired(kv.expire_ts) {
			let write_opts = WriteOptions {
				await_durable: false,
			};
			db.delete_with_options(meta_encoded_key, &write_opts)
				.await?;
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
		if meta_val.version() == Some(0) {
			meta_val.set_version(kv.seq);
		}

		let logical_expire_time = meta_val.expire_time();
		if logical_expire_time == 0 {
			if let Some(ts) = kv.expire_ts {
				meta_val.set_expire_time(ts.max(0) as u64);
			}
		} else {
			let logical_expire_ts = i64::try_from(logical_expire_time).map_err(|_| {
				StorageError::DataInconsistency {
					message: "metadata expiration exceeds SlateDB timestamp range".to_string(),
				}
			})?;
			if is_expired(Some(logical_expire_ts)) {
				let write_opts = WriteOptions {
					await_durable: false,
				};
				db.delete_with_options(meta_encoded_key, &write_opts)
					.await?;
				return Ok(None);
			}
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
		use crate::string::meta::HashMetaValue;
		use crate::string::meta::ListMetaValue;
		use crate::string::meta::SetMetaValue;
		use crate::string::meta::ZSetMetaValue;

		async fn move_meta_to_legacy<T: MetaValue>(
			source: &Db,
			string_db: &Db,
			key: &Bytes,
			delete_source: bool,
		) -> u64 {
			let meta = Storage::get_meta_from_db::<T>(source, key)
				.await
				.unwrap()
				.unwrap();
			let encoded_key = MetaKey::new(key.clone()).encode();
			let write_opts = WriteOptions {
				await_durable: false,
			};
			let put_opts = Storage::meta_put_opts(&meta);
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
				&storage.hash_db,
				&storage.string_db,
				&hash_key,
				false,
			)
			.await,
			move_meta_to_legacy::<ListMetaValue>(
				&storage.list_db,
				&storage.string_db,
				&list_key,
				true,
			)
			.await,
			move_meta_to_legacy::<SetMetaValue>(
				&storage.set_db,
				&storage.string_db,
				&set_key,
				true,
			)
			.await,
			move_meta_to_legacy::<ZSetMetaValue>(
				&storage.zset_db,
				&storage.string_db,
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
			let encoded_key = MetaKey::new(key).encode();
			assert!(
				storage
					.string_db
					.get(encoded_key.clone())
					.await
					.unwrap()
					.is_none()
			);
			let migrated = storage
				.db_for_type(data_type)
				.get_key_value(encoded_key)
				.await
				.unwrap()
				.unwrap();
			let normalized = Storage::normalized_top_level(&migrated).unwrap();
			assert_eq!(normalized.logical_expire_ts, Some(expire_time as i64));
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
	async fn test_startup_recovers_string_from_hash_only_candidate_layout() {
		use crate::string::value::StringValue;

		let timestamp = ulid::Ulid::generate().to_string();
		let path = std::env::temp_dir().join(format!("nimbis_string_recovery_{timestamp}"));
		std::fs::create_dir_all(&path).unwrap();
		let storage = Storage::open(&path, None).await.unwrap();
		let key = Bytes::from_static(b"misplaced-string");
		let encoded_key = MetaKey::new(key.clone()).encode();
		storage
			.hash_db
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
				.get(encoded_key.clone())
				.await
				.unwrap()
				.is_none()
		);
		assert!(storage.string_db.get(encoded_key).await.unwrap().is_some());
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
		let encoded_key = MetaKey::new(key.clone()).encode();
		let encoded_value = StringValue::new("value").encode();
		let put_opts = PutOptions {
			ttl: Ttl::ExpireAfter(120_000),
		};
		let write_opts = WriteOptions {
			await_durable: false,
		};

		storage
			.hash_db
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
			.get_key_value(encoded_key.clone())
			.await
			.unwrap()
			.unwrap()
			.expire_ts
			.unwrap();
		storage
			.string_db
			.put_with_options(encoded_key.clone(), encoded_value, &put_opts, &write_opts)
			.await
			.unwrap();
		let destination_expire_ts = storage
			.string_db
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
				.get(encoded_key.clone())
				.await
				.unwrap()
				.is_none()
		);
		assert_eq!(
			storage
				.string_db
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
		let encoded_key = MetaKey::new("skip-migration-sentinel").encode();
		let invalid_hash_meta = Bytes::from(vec![DataType::Hash as u8]);
		storage
			.string_db
			.put(encoded_key.clone(), invalid_hash_meta.clone())
			.await
			.unwrap();
		storage.close().await.unwrap();
		drop(storage);

		let storage = Storage::open(&path, None).await.unwrap();
		assert_eq!(
			storage.string_db.get(encoded_key).await.unwrap(),
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
		let write_opts = WriteOptions {
			await_durable: false,
		};
		for index in 0..count {
			let key = Bytes::from(format!("chunked-string-{index:04}"));
			storage
				.hash_db
				.put_with_options(
					MetaKey::new(key).encode(),
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
			let encoded_key = MetaKey::new(key.clone()).encode();
			assert_eq!(
				storage.get(key).await.unwrap(),
				Some(Bytes::from(format!("value-{index}")))
			);
			assert!(storage.hash_db.get(encoded_key).await.unwrap().is_none());
		}
		storage.close().await.unwrap();
		drop(storage);
		let _ = std::fs::remove_dir_all(path);
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
