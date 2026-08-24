use std::marker::PhantomData;
use std::sync::Arc;

use bytes::Bytes;
use slatedb::ByteRangeBounds;
use slatedb::Db;
use slatedb::DbIterator;
use slatedb::KeyValue;
use slatedb::WriteBatch;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;
#[cfg(test)]
use slatedb_common::metrics::DefaultMetricsRecorder;
#[cfg(test)]
use slatedb_common::metrics::lookup_metric;

use crate::error::StorageError;
use crate::expiration::ttl_for_expiration;
use crate::string::meta::CollectionMeta;
use crate::string::meta::TopLevelValue;
use crate::string::value::StringValue;
use crate::top_level_key::TopLevelKey;
use crate::top_level_row::normalize_top_level_row;

/// A physical database whose top-level rows always decode as `V`.
///
/// Collection metadata and sub-key access goes through this type so callers
/// cannot pair a value type with the wrong physical database. Raw access is
/// reserved for lifecycle, migration, and explicit on-disk-state tests.
pub(crate) struct TypedDb<V> {
	db: Arc<Db>,
	#[cfg(test)]
	metrics: Arc<DefaultMetricsRecorder>,
	value: PhantomData<fn() -> V>,
}

impl<V> Clone for TypedDb<V> {
	fn clone(&self) -> Self {
		Self {
			db: self.db.clone(),
			#[cfg(test)]
			metrics: self.metrics.clone(),
			value: PhantomData,
		}
	}
}

impl<V> TypedDb<V> {
	pub(crate) fn new(db: Arc<Db>, #[cfg(test)] metrics: Arc<DefaultMetricsRecorder>) -> Self {
		Self {
			db,
			#[cfg(test)]
			metrics,
			value: PhantomData,
		}
	}

	/// Access the physical database for migration, keyspace lifecycle work, and
	/// tests that intentionally construct invalid on-disk state. Collection
	/// command paths must use the typed read APIs and `commit` instead.
	pub(crate) fn raw(&self) -> &Db {
		&self.db
	}

	#[cfg(test)]
	pub(crate) fn metric(&self, name: &'static str) -> i64 {
		lookup_metric(&self.metrics, name)
			.unwrap_or_else(|| panic!("missing SlateDB metric {name}"))
	}
}

impl<V: TopLevelValue> TypedDb<V> {
	pub(crate) async fn load_value(&self, key: &Bytes) -> Result<Option<V>, StorageError> {
		self.load_row(key).await
	}

	async fn load_row(&self, key: &Bytes) -> Result<Option<V>, StorageError> {
		let key = TopLevelKey::new(key.clone())?;
		let encoded_key = key.encode();
		let kv = match self.db.get_key_value(encoded_key.clone()).await? {
			Some(kv) => kv,
			None => return Ok(None),
		};

		let normalized =
			normalize_top_level_row::<V>(&kv.value, kv.seq, kv.expire_ts, V::DATA_TYPE)?;
		if normalized.is_expired() {
			self.delete_top_level(encoded_key).await?;
			return Ok(None);
		}

		Ok(Some(normalized.value))
	}

	async fn delete_top_level(&self, encoded_key: Bytes) -> Result<(), StorageError> {
		let write_options = WriteOptions::default();
		self.db
			.delete_with_options(encoded_key, &write_options)
			.await?;
		Ok(())
	}
}

impl TypedDb<StringValue> {
	/// Store one string top-level value. Strings have no dependent sub-keys, so
	/// this is their only write path outside lifecycle operations.
	pub(crate) async fn store(
		&self,
		key: TopLevelKey,
		value: StringValue,
	) -> Result<(), StorageError> {
		let write_options = WriteOptions::default();
		self.db
			.put_with_options(
				key.encode(),
				value.encode(),
				&PutOptions::default(),
				&write_options,
			)
			.await?;
		Ok(())
	}
}

pub(crate) enum MetadataChange<M> {
	Put(M),
	Delete,
}

impl<M: CollectionMeta> TypedDb<M> {
	/// Read a physical sub-key while retaining an explicit typed-database
	/// boundary at collection call sites.
	pub(crate) async fn get_entry<K: AsRef<[u8]> + Send>(
		&self,
		key: K,
	) -> Result<Option<KeyValue>, StorageError> {
		Ok(self.db.get_key_value(key).await?)
	}

	/// Scan physical sub-keys belonging to this collection database.
	pub(crate) async fn scan_entries<T>(&self, range: T) -> Result<DbIterator, StorageError>
	where
		T: ByteRangeBounds + Send,
	{
		Ok(self.db.scan(range).await?)
	}

	/// Scan physical sub-keys sharing a prefix in this collection database.
	pub(crate) async fn scan_entry_prefix<P: AsRef<[u8]> + Send>(
		&self,
		prefix: P,
	) -> Result<DbIterator, StorageError> {
		Ok(self.db.scan_prefix(prefix, ..).await?)
	}

	pub(crate) async fn load(&self, key: &Bytes) -> Result<Option<M>, StorageError> {
		self.load_row(key).await
	}

	/// Commit sub-key writes and the corresponding metadata transition with one
	/// SlateDB sequence number.
	pub(crate) async fn commit(
		&self,
		key: &Bytes,
		mut batch: WriteBatch,
		metadata: MetadataChange<M>,
	) -> Result<(), StorageError> {
		let metadata_key = TopLevelKey::new(key.clone())?.encode();
		match metadata {
			MetadataChange::Put(metadata) => {
				let put_options = metadata_put_options(&metadata)?;
				batch.put_with_options(metadata_key, metadata.encode(), &put_options);
			}
			MetadataChange::Delete => batch.delete(metadata_key),
		}

		if batch.is_empty() {
			return Ok(());
		}
		let write_options = WriteOptions::default();
		self.db.write_with_options(batch, &write_options).await?;
		Ok(())
	}
}

pub(crate) fn metadata_put_options(value: &impl TopLevelValue) -> Result<PutOptions, StorageError> {
	Ok(PutOptions {
		ttl: ttl_for_expiration(value.expire_time())?,
	})
}
