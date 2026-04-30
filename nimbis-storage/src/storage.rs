use std::sync::Arc;

use bytes::Bytes;
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
use crate::string::meta::MetaKey;
use crate::string::meta::MetaValue;
use crate::utils::is_expired;

#[derive(Clone)]
pub struct Storage {
	pub(crate) string_db: Arc<Db>,
	pub(crate) hash_db: Arc<Db>,
	pub(crate) list_db: Arc<Db>,
	pub(crate) set_db: Arc<Db>,
	pub(crate) zset_db: Arc<Db>,
}

fn shard_path(base_path: ObjectStorePath, shard_id: Option<usize>) -> ObjectStorePath {
	match shard_id {
		Some(id) => base_path.child(format!("shard-{}", id)),
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
		}
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
		object_store
			.put(&marker, bytes::Bytes::new().into())
			.await
			.map_err(StorageError::from)?;

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

		// Open collection DBs with CollectionCompactionFilter referencing string_db
		let open_db_with_collection_filter = |name: &'static str, data_type: DataType| {
			let store = object_store.clone();
			let cache = cache.clone();
			let string_db = string_db.clone();
			let db_path = child_path(name);
			async move {
				let compactor_builder =
					slatedb::CompactorBuilder::new(db_path.clone(), store.clone())
						.with_compaction_filter_supplier(Arc::new(
							CollectionCompactionFilterSupplier {
								string_db,
								data_type,
							},
						));
				let db: Result<Db, slatedb::Error> = Db::builder(db_path, store)
					.with_db_cache(cache)
					.with_compactor_builder(compactor_builder)
					.build()
					.await;
				db.map_err(StorageError::from)
			}
		};

		let (hash_db, list_db, set_db, zset_db) = tokio::try_join!(
			open_db_with_collection_filter("hash", DataType::Hash),
			open_db_with_collection_filter("list", DataType::List),
			open_db_with_collection_filter("set", DataType::Set),
			open_db_with_collection_filter("zset", DataType::ZSet)
		)?;

		Ok(Self::new(
			string_db,
			Arc::new(hash_db),
			Arc::new(list_db),
			Arc::new(set_db),
			Arc::new(zset_db),
		))
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
		let meta_encoded_key = meta_key.encode();
		let kv = match self
			.string_db
			.get_key_value(meta_encoded_key.clone())
			.await?
		{
			Some(kv) => kv,
			None => return Ok(None),
		};

		if is_expired(kv.expire_ts) {
			let write_opts = WriteOptions {
				await_durable: false,
			};
			self.string_db
				.delete_with_options(meta_encoded_key, &write_opts)
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

		use crate::compaction_filter::CollectionCompactionFilter;
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
		let mut filter = CollectionCompactionFilter {
			string_db: ctx.storage.string_db.clone(),
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
