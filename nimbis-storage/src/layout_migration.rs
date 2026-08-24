use std::ops::Bound;

use bytes::Bytes;
use slatedb::Db;
use slatedb::config::PutOptions;
use slatedb::config::WriteOptions;
use slatedb::object_store::ObjectStore;
use slatedb::object_store::path::Path as ObjectStorePath;

use crate::data_type::DataType;
use crate::error::StorageError;
use crate::expiration::ttl_for_expiration;
use crate::storage::Storage;
use crate::string::meta::AnyValue;
use crate::top_level_key::TopLevelKey;
use crate::top_level_row::NormalizedTopLevelRow;
use crate::top_level_row::normalize_top_level_row;

const CURRENT_LAYOUT_VERSION: &[u8] = b"nimbis-layout:type-local-metadata:v1\n";
const MIGRATION_SCAN_CHUNK_SIZE: usize = 64;
const MAX_MIGRATION_TTL_DRIFT_MS: i64 = 5_000;

type NormalizedTopLevel = NormalizedTopLevelRow<AnyValue>;

/// Brings legacy metadata locations into the type-local layout before the
/// returned [`Storage`] can be used by the server.
pub(crate) async fn ensure_current_layout(
	storage: &Storage,
	object_store: &dyn ObjectStore,
	marker: &ObjectStorePath,
) -> Result<(), StorageError> {
	if layout_marker_is_current(object_store, marker).await? {
		return Ok(());
	}

	// The migration is intentionally durable and idempotent. Databases created
	// by the old split-metadata layout converge to a single local authority
	// without a runtime dual-read fallback.
	migrate_legacy_layout(storage).await?;

	// Publishing the version is the migration commit point. Every destination
	// write and source delete is durable before this marker is replaced, so an
	// empty/old marker always remains safe to retry after interruption.
	object_store
		.put(marker, Bytes::from_static(CURRENT_LAYOUT_VERSION).into())
		.await
		.map_err(StorageError::from)?;
	Ok(())
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

fn migration_put_options(logical_expire_ts: Option<i64>) -> Result<PutOptions, StorageError> {
	let ttl = match logical_expire_ts {
		Some(expire_ts) => ttl_for_expiration(expire_ts.max(0) as u64)?,
		None => slatedb::config::Ttl::NoExpiry,
	};
	Ok(PutOptions { ttl })
}

async fn copy_top_level_durably(
	source_db: &Db,
	destination_db: &Db,
	encoded_key: Bytes,
	source_kv: slatedb::KeyValue,
	expected_type: DataType,
) -> Result<(), StorageError> {
	let durable = WriteOptions {
		await_durable: true,
	};
	let source_value = normalize_top_level_row::<AnyValue>(
		&source_kv.value,
		source_kv.seq,
		source_kv.expire_ts,
		expected_type,
	)?;
	if source_value.is_expired() {
		source_db.delete_with_options(encoded_key, &durable).await?;
		return Ok(());
	}
	let normalized_source = source_value.value.encode();

	if let Some(destination_kv) = destination_db.get_key_value(encoded_key.clone()).await? {
		let destination_value = normalize_top_level_row::<AnyValue>(
			&destination_kv.value,
			destination_kv.seq,
			destination_kv.expire_ts,
			expected_type,
		)?;
		if !destination_matches_source(&source_value, &destination_value, expected_type) {
			return Err(StorageError::DataInconsistency {
				message: format!(
					"conflicting {expected_type:?} metadata authorities during layout migration"
				),
			});
		}
	} else {
		let put_options = migration_put_options(source_value.logical_expire_ts)?;
		destination_db
			.put_with_options(
				encoded_key.clone(),
				normalized_source.clone(),
				&put_options,
				&durable,
			)
			.await?;

		let Some(destination_kv) = destination_db.get_key_value(encoded_key.clone()).await? else {
			// A key can expire while it is being migrated. The source remains a
			// valid recovery authority unless it is now expired as well.
			if source_value.is_expired() {
				source_db.delete_with_options(encoded_key, &durable).await?;
				return Ok(());
			}
			return Err(StorageError::DataInconsistency {
				message: format!(
					"{expected_type:?} metadata was not visible after durable migration write"
				),
			});
		};
		let destination_value = normalize_top_level_row::<AnyValue>(
			&destination_kv.value,
			destination_kv.seq,
			destination_kv.expire_ts,
			expected_type,
		)?;
		if !destination_matches_source(&source_value, &destination_value, expected_type) {
			// This destination did not exist before this invocation. Remove an
			// incompatible copy so an old marker can retry instead of becoming
			// permanently wedged on the next startup.
			destination_db
				.delete_with_options(encoded_key.clone(), &durable)
				.await?;
			return Err(StorageError::DataInconsistency {
				message: format!("{expected_type:?} metadata verification failed after migration"),
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
	storage: &Storage,
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
			let encoded_type = kv.value.first().and_then(|value| DataType::from_u8(*value));
			if TopLevelKey::decode_exact(&kv.key).is_err() {
				// Collection DBs also contain sub-keys, so a non-exact key is normal
				// there. The legacy String DB, however, contains only top-level
				// rows. A collection metadata value with a non-exact key is malformed
				// and cannot be assigned to one logical key unambiguously. Publishing
				// the new marker would then make that collection permanently invisible.
				if source_type == DataType::String
					&& encoded_type.is_some_and(|data_type| data_type != DataType::String)
				{
					return Err(StorageError::DataInconsistency {
						message: format!(
							"cannot safely migrate legacy collection metadata with an invalid top-level key length (encoded bytes: {})",
							kv.key.len()
						),
					});
				}
				continue;
			}
			let Some(encoded_type) = encoded_type else {
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
			copy_top_level_durably(
				source_db,
				storage.raw_db_for_type(destination_type),
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

async fn migrate_legacy_layout(storage: &Storage) -> Result<(), StorageError> {
	// First recover Strings written into hash_db by the earlier hash-only
	// co-location candidate. New writes never place Strings in collection DBs.
	for (data_type, db) in storage.collection_raw_dbs() {
		migrate_legacy_source(storage, data_type, db).await?;
	}

	// Then move legacy collection metadata out of string_db. Each scan is
	// bounded and dropped before its source rows are durably deleted.
	migrate_legacy_source(storage, DataType::String, storage.string_db.raw()).await?;
	Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
	use crate::error::StorageError;

	pub(crate) const CURRENT_LAYOUT_VERSION: &[u8] = super::CURRENT_LAYOUT_VERSION;
	pub(crate) const MAX_MIGRATION_TTL_DRIFT_MS: i64 = super::MAX_MIGRATION_TTL_DRIFT_MS;
	pub(crate) const MIGRATION_SCAN_CHUNK_SIZE: usize = super::MIGRATION_SCAN_CHUNK_SIZE;

	pub(crate) fn logical_expire_ts(kv: &slatedb::KeyValue) -> Result<Option<i64>, StorageError> {
		let expected_type = kv
			.value
			.first()
			.and_then(|type_code| crate::data_type::DataType::from_u8(*type_code))
			.ok_or_else(|| StorageError::DataInconsistency {
				message: "top-level row has an invalid type tag".to_string(),
			})?;
		Ok(
			super::normalize_top_level_row::<crate::string::meta::AnyValue>(
				&kv.value,
				kv.seq,
				kv.expire_ts,
				expected_type,
			)?
			.logical_expire_ts,
		)
	}
}
