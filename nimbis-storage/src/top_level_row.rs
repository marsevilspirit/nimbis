use crate::data_type::DataType;
use crate::error::StorageError;
use crate::expiration::is_expired;
use crate::expiration::validate_expiration;
use crate::expiration::validate_row_expiration;
use crate::string::meta::TopLevelState;

/// A decoded top-level row with its physical and logical state reconciled.
///
/// This type deliberately does not perform lazy deletion. Readers and the
/// layout migrator decide what to do with an expired row after normalization.
pub(crate) struct NormalizedTopLevelRow<V> {
	pub(crate) value: V,
	pub(crate) logical_expire_ts: Option<i64>,
	pub(crate) row_expire_ts: Option<i64>,
}

impl<V> NormalizedTopLevelRow<V> {
	pub(crate) fn is_expired(&self) -> bool {
		is_expired(self.row_expire_ts) || is_expired(self.logical_expire_ts)
	}
}

/// Decode and reconcile one top-level SlateDB row.
///
/// Collection metadata uses its embedded deadline as the logical source of
/// truth. A legacy collection row with no embedded deadline inherits the row
/// deadline, while Strings always use the row deadline. Row deadlines and
/// embedded deadlines intentionally use their separate validation ceilings.
pub(crate) fn normalize_top_level_row<V: TopLevelState>(
	encoded_value: &[u8],
	row_sequence: u64,
	row_expire_ts: Option<i64>,
	expected_type: DataType,
) -> Result<NormalizedTopLevelRow<V>, StorageError> {
	validate_row_expiration(row_expire_ts)?;

	let encoded_type = encoded_value
		.first()
		.and_then(|type_code| DataType::from_u8(*type_code));
	if encoded_type != Some(expected_type) {
		return Err(StorageError::DataInconsistency {
			message: format!("top-level row expected {expected_type:?}, found {encoded_type:?}"),
		});
	}

	let mut value = V::decode_state(encoded_value)?;
	if value.data_type() != expected_type {
		return Err(StorageError::DataInconsistency {
			message: format!(
				"top-level decoder expected {expected_type:?}, produced {:?}",
				value.data_type()
			),
		});
	}
	value.resolve_pending_generation(row_sequence);

	let logical_expire_ts = match value.embedded_expire_time() {
		None => row_expire_ts,
		Some(0) => {
			if let Some(expire_ts) = row_expire_ts {
				// `validate_row_expiration` has already established non-negativity.
				let expire_time = expire_ts as u64;
				// Once inherited by collection metadata this becomes a logical
				// deadline and must satisfy the stricter logical ceiling as well.
				validate_expiration(expire_time)?;
				value.set_embedded_expire_time(expire_time);
			}
			row_expire_ts
		}
		Some(expire_time) => {
			validate_expiration(expire_time)?;
			Some(
				i64::try_from(expire_time).map_err(|_| StorageError::DataInconsistency {
					message: "metadata expiration exceeds SlateDB timestamp range".to_string(),
				})?,
			)
		}
	};

	Ok(NormalizedTopLevelRow {
		value,
		logical_expire_ts,
		row_expire_ts,
	})
}

#[cfg(test)]
mod tests {
	use bytes::Bytes;

	use super::*;
	use crate::expiration::MAX_EXPIRATION_TIMESTAMP_MS;
	use crate::expiration::MAX_ROW_EXPIRATION_TIMESTAMP_MS;
	use crate::string::meta::HashMetaValue;
	use crate::string::value::StringValue;

	#[test]
	fn resolves_pending_generation_and_inherits_legacy_row_deadline() {
		let encoded = HashMetaValue::new(0, 2).encode();
		let normalized =
			normalize_top_level_row::<HashMetaValue>(&encoded, 42, Some(123_456), DataType::Hash)
				.unwrap();

		assert_eq!(normalized.value.version, 42);
		assert_eq!(normalized.value.expire_time, 123_456);
		assert_eq!(normalized.logical_expire_ts, Some(123_456));
		assert_eq!(normalized.row_expire_ts, Some(123_456));
	}

	#[test]
	fn string_uses_the_row_deadline_without_embedding_it() {
		let encoded = StringValue::new(Bytes::from_static(b"value")).encode();
		let normalized = normalize_top_level_row::<StringValue>(
			&encoded,
			7,
			Some(MAX_ROW_EXPIRATION_TIMESTAMP_MS as i64),
			DataType::String,
		)
		.unwrap();

		assert_eq!(normalized.logical_expire_ts, normalized.row_expire_ts);
		assert_eq!(normalized.value.value, Bytes::from_static(b"value"));
	}

	#[test]
	fn keeps_logical_and_row_expiration_limits_distinct() {
		let encoded = HashMetaValue::new_with_ttl(1, 1, MAX_EXPIRATION_TIMESTAMP_MS).encode();
		let normalized = normalize_top_level_row::<HashMetaValue>(
			&encoded,
			1,
			Some(MAX_ROW_EXPIRATION_TIMESTAMP_MS as i64),
			DataType::Hash,
		)
		.unwrap();
		assert_eq!(
			normalized.logical_expire_ts,
			Some(MAX_EXPIRATION_TIMESTAMP_MS as i64)
		);
		assert_eq!(
			normalized.row_expire_ts,
			Some(MAX_ROW_EXPIRATION_TIMESTAMP_MS as i64)
		);

		let encoded = HashMetaValue::new(1, 1).encode();
		assert!(matches!(
			normalize_top_level_row::<HashMetaValue>(
				&encoded,
				1,
				Some(MAX_ROW_EXPIRATION_TIMESTAMP_MS as i64),
				DataType::Hash,
			),
			Err(StorageError::InvalidExpiration {
				timestamp: MAX_ROW_EXPIRATION_TIMESTAMP_MS,
				max: MAX_EXPIRATION_TIMESTAMP_MS,
			})
		));

		let encoded = HashMetaValue::new_with_ttl(1, 1, MAX_EXPIRATION_TIMESTAMP_MS + 1).encode();
		assert!(matches!(
			normalize_top_level_row::<HashMetaValue>(&encoded, 1, None, DataType::Hash),
			Err(StorageError::InvalidExpiration {
				timestamp,
				max: MAX_EXPIRATION_TIMESTAMP_MS,
			}) if timestamp == MAX_EXPIRATION_TIMESTAMP_MS + 1
		));

		let encoded = StringValue::new(Bytes::new()).encode();
		assert!(matches!(
			normalize_top_level_row::<StringValue>(
				&encoded,
				1,
				Some((MAX_ROW_EXPIRATION_TIMESTAMP_MS + 1) as i64),
				DataType::String,
			),
			Err(StorageError::InvalidExpiration {
				timestamp,
				max: MAX_ROW_EXPIRATION_TIMESTAMP_MS,
			}) if timestamp == MAX_ROW_EXPIRATION_TIMESTAMP_MS + 1
		));
	}

	#[test]
	fn rejects_a_row_from_the_wrong_typed_database() {
		let encoded = StringValue::new(Bytes::new()).encode();
		assert!(matches!(
			normalize_top_level_row::<StringValue>(&encoded, 1, None, DataType::Hash),
			Err(StorageError::DataInconsistency { .. })
		));
	}
}
