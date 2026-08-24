use std::collections::HashSet;

use bytes::Bytes;
use chrono::Utc;
use nimbis_macros::storage_lock;
use slatedb::Db;
use slatedb::WriteBatch;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;

use crate::data_type::DataType;
use crate::error::StorageError;
use crate::expiration::ttl_for_expiration;
use crate::expiration::validate_expiration;
use crate::storage::Storage;
use crate::string::meta::AnyValue;
use crate::top_level_key::TopLevelKey;
use crate::top_level_row::normalize_top_level_row;

struct TypedNamespace<'a> {
	data_type: DataType,
	db: &'a Db,
}

struct LoadedTopLevel {
	encoded_key: Bytes,
	value: AnyValue,
	logical_expire_ts: Option<i64>,
}

impl<'a> TypedNamespace<'a> {
	fn new(storage: &'a Storage, data_type: DataType) -> Self {
		Self {
			data_type,
			db: storage.raw_db_for_type(data_type),
		}
	}

	async fn load_live(&self, key: &Bytes) -> Result<Option<LoadedTopLevel>, StorageError> {
		let encoded_key = TopLevelKey::new(key.clone())?.encode();
		let Some(row) = self.db.get_key_value(encoded_key.clone()).await? else {
			return Ok(None);
		};
		let normalized = normalize_top_level_row::<AnyValue>(
			&row.value,
			row.seq,
			row.expire_ts,
			self.data_type,
		)?;
		if normalized.is_expired() {
			let write_options = WriteOptions::default();
			self.db
				.delete_with_options(encoded_key, &write_options)
				.await?;
			return Ok(None);
		}
		Ok(Some(LoadedTopLevel {
			encoded_key,
			value: normalized.value,
			logical_expire_ts: normalized.logical_expire_ts,
		}))
	}
}

impl Storage {
	#[storage_lock(write_many, keys, data_type)]
	#[fastrace::trace]
	pub async fn del<I>(&self, data_type: DataType, keys: I) -> Result<i64, StorageError>
	where
		I: IntoIterator<Item = Bytes>,
	{
		let namespace = TypedNamespace::new(self, data_type);
		let mut batch = WriteBatch::new();
		let mut deleted = 0;
		let mut seen = HashSet::new();

		for key in keys {
			let encoded_key = TopLevelKey::new(key.clone())?.encode();
			if !seen.insert(encoded_key) {
				continue;
			}
			let Some(row) = namespace.load_live(&key).await? else {
				continue;
			};
			deleted += 1;
			batch.delete(row.encoded_key);
		}

		if !batch.is_empty() {
			let write_options = WriteOptions::default();
			namespace
				.db
				.write_with_options(batch, &write_options)
				.await?;
		}
		Ok(deleted)
	}

	#[storage_lock(write, key, data_type)]
	#[fastrace::trace]
	pub async fn expire(
		&self,
		data_type: DataType,
		key: Bytes,
		expire_time: u64,
	) -> Result<bool, StorageError> {
		validate_expiration(expire_time)?;
		let namespace = TypedNamespace::new(self, data_type);
		let Some(row) = namespace.load_live(&key).await? else {
			return Ok(false);
		};
		let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
		let write_options = WriteOptions::default();
		if expire_time > 0 && expire_time <= now {
			namespace
				.db
				.delete_with_options(row.encoded_key, &write_options)
				.await?;
			return Ok(true);
		}

		let mut value = row.value;
		value.set_expire_time(expire_time);
		let ttl = ttl_for_expiration(expire_time)?;
		namespace
			.db
			.put_with_options(
				row.encoded_key,
				value.encode(),
				&PutOptions { ttl },
				&write_options,
			)
			.await?;
		Ok(true)
	}

	#[storage_lock(read, key, data_type)]
	#[fastrace::trace]
	pub async fn ttl(&self, data_type: DataType, key: Bytes) -> Result<Option<i64>, StorageError> {
		let namespace = TypedNamespace::new(self, data_type);
		let Some(row) = namespace.load_live(&key).await? else {
			return Ok(None);
		};
		Ok(Some(match row.logical_expire_ts {
			Some(expire_ts) => (expire_ts - Utc::now().timestamp_millis()).max(0),
			None => -1,
		}))
	}

	#[storage_lock(read, key, data_type)]
	#[fastrace::trace]
	pub async fn exists(&self, data_type: DataType, key: Bytes) -> Result<bool, StorageError> {
		Ok(TypedNamespace::new(self, data_type)
			.load_live(&key)
			.await?
			.is_some())
	}

	#[storage_lock(read_many, keys, data_type)]
	#[fastrace::trace]
	pub async fn exists_many<I>(&self, data_type: DataType, keys: I) -> Result<i64, StorageError>
	where
		I: IntoIterator<Item = Bytes>,
	{
		let namespace = TypedNamespace::new(self, data_type);
		let mut count = 0;
		for key in keys {
			if namespace.load_live(&key).await?.is_some() {
				count += 1;
			}
		}
		Ok(count)
	}
}
