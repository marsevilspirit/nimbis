use slatedb::config::Ttl;

use crate::error::StorageError;

pub(crate) const MAX_ROW_EXPIRATION_TIMESTAMP_MS: u64 = 253_402_300_799_999;
const SLATEDB_CLOCK_HEADROOM_MS: u64 = 86_400_000;

/// Latest supported logical deadline, one day before year 10000.
///
/// SlateDB derives an absolute deadline by adding `ExpireAfter` to a signed
/// millisecond clock at a later instant. The explicit headroom absorbs that
/// second clock read without allowing its resulting row deadline past the
/// practical timestamp ceiling.
pub(crate) const MAX_EXPIRATION_TIMESTAMP_MS: u64 =
	MAX_ROW_EXPIRATION_TIMESTAMP_MS - SLATEDB_CLOCK_HEADROOM_MS;

pub(crate) fn validate_expiration(timestamp: u64) -> Result<(), StorageError> {
	if timestamp > MAX_EXPIRATION_TIMESTAMP_MS {
		return Err(StorageError::InvalidExpiration {
			timestamp,
			max: MAX_EXPIRATION_TIMESTAMP_MS,
		});
	}
	Ok(())
}

pub(crate) fn validate_row_expiration(timestamp: Option<i64>) -> Result<(), StorageError> {
	let Some(timestamp) = timestamp else {
		return Ok(());
	};
	let timestamp = u64::try_from(timestamp).map_err(|_| StorageError::DataInconsistency {
		message: "SlateDB row contains a negative expiration timestamp".to_string(),
	})?;
	if timestamp > MAX_ROW_EXPIRATION_TIMESTAMP_MS {
		return Err(StorageError::InvalidExpiration {
			timestamp,
			max: MAX_ROW_EXPIRATION_TIMESTAMP_MS,
		});
	}
	Ok(())
}

pub(crate) fn ttl_for_expiration(timestamp: u64) -> Result<Ttl, StorageError> {
	if timestamp == 0 {
		return Ok(Ttl::NoExpiry);
	}
	validate_expiration(timestamp)?;
	let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
	Ok(Ttl::ExpireAfter(timestamp.saturating_sub(now)))
}

pub(crate) fn is_expired(timestamp: Option<i64>) -> bool {
	timestamp.is_some_and(|timestamp| timestamp <= chrono::Utc::now().timestamp_millis())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn validates_the_supported_expiration_boundary() {
		assert!(validate_expiration(0).is_ok());
		assert!(validate_expiration(MAX_EXPIRATION_TIMESTAMP_MS).is_ok());
		assert!(matches!(
			validate_expiration(MAX_EXPIRATION_TIMESTAMP_MS + 1),
			Err(StorageError::InvalidExpiration {
				timestamp,
				max: MAX_EXPIRATION_TIMESTAMP_MS,
			}) if timestamp == MAX_EXPIRATION_TIMESTAMP_MS + 1
		));
	}

	#[test]
	fn validates_the_row_expiration_headroom_boundary() {
		assert!(validate_row_expiration(None).is_ok());
		assert!(validate_row_expiration(Some(MAX_ROW_EXPIRATION_TIMESTAMP_MS as i64)).is_ok());
		assert!(matches!(
			validate_row_expiration(Some((MAX_ROW_EXPIRATION_TIMESTAMP_MS + 1) as i64)),
			Err(StorageError::InvalidExpiration { .. })
		));
	}

	#[test]
	fn converts_zero_and_expired_deadlines_without_overflow() {
		assert_eq!(ttl_for_expiration(0).unwrap(), Ttl::NoExpiry);
		assert_eq!(ttl_for_expiration(1).unwrap(), Ttl::ExpireAfter(0));
		assert!(matches!(
			ttl_for_expiration(MAX_EXPIRATION_TIMESTAMP_MS).unwrap(),
			Ttl::ExpireAfter(duration) if duration > 0
		));
	}
}
