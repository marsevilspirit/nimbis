use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecoderError {
	#[error("Empty key, cannot decode")]
	Empty,
	#[error("Invalid type code")]
	InvalidType,
	#[error("Invalid data length")]
	InvalidLength,
}

impl DecoderError {
	/// Returns the unique error code for this error variant
	pub fn code(&self) -> &'static str {
		match self {
			Self::Empty => "E0001",
			Self::InvalidType => "E0002",
			Self::InvalidLength => "E0003",
		}
	}
}

#[derive(Debug, Error)]
pub enum StorageError {
	/// Database operation failed
	#[error("Database operation failed: {source}")]
	DatabaseError {
		#[source]
		source: Box<dyn std::error::Error + Send + Sync>,
	},

	/// Encoding/Decoding error
	#[error("Failed to decode data: {source}")]
	DecodeError {
		#[source]
		source: DecoderError,
	},

	/// I/O operation failed
	#[error("I/O operation failed: {source}")]
	IoError {
		#[source]
		source: std::io::Error,
	},

	/// Data inconsistency detected
	#[error("Data inconsistency detected: {message}")]
	DataInconsistency { message: String },

	/// Object store configuration failed
	#[error("Object store configuration failed: {message}")]
	ObjectStoreConfig { message: String },

	/// The user key cannot be represented by the on-disk key codec.
	#[error("Key length {length} exceeds the supported maximum of {max} bytes")]
	InvalidKeyLength { length: usize, max: usize },

	/// The absolute expiration exceeds Nimbis's supported logical deadline.
	#[error("Expiration timestamp {timestamp} exceeds the supported maximum of {max} milliseconds")]
	InvalidExpiration { timestamp: u64, max: u64 },
}

impl StorageError {
	/// Returns the error code for this error variant
	pub fn code(&self) -> &'static str {
		match self {
			Self::DatabaseError { .. } => "E1000",
			Self::DecodeError { .. } => "E1002",
			Self::IoError { .. } => "E1003",
			Self::DataInconsistency { .. } => "E1004",
			Self::ObjectStoreConfig { .. } => "E1005",
			Self::InvalidKeyLength { .. } => "E1006",
			Self::InvalidExpiration { .. } => "E1007",
		}
	}

	/// Returns detailed error code including nested error codes
	/// For example: "E1002:E0001" for DecodeError wrapping DecoderError::Empty
	pub fn detailed_code(&self) -> String {
		match self {
			Self::DecodeError { source } => {
				format!("{}:{}", self.code(), source.code())
			}
			_ => self.code().to_string(),
		}
	}
}

// Auto-convert from DecoderError
impl From<DecoderError> for StorageError {
	fn from(err: DecoderError) -> Self {
		Self::DecodeError { source: err }
	}
}

// Auto-convert from std::io::Error
impl From<std::io::Error> for StorageError {
	fn from(err: std::io::Error) -> Self {
		Self::IoError { source: err }
	}
}

// Convert from boxed errors (mainly from slatedb)
impl From<Box<dyn std::error::Error + Send + Sync>> for StorageError {
	fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
		// Check if it's an io::Error
		if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
			// Clone the io::Error to avoid ownership issues
			return Self::IoError {
				source: std::io::Error::new(io_err.kind(), io_err.to_string()),
			};
		}

		Self::DatabaseError { source: err }
	}
}

// Convert from slatedb errors
impl From<slatedb::Error> for StorageError {
	fn from(err: slatedb::Error) -> Self {
		Self::DatabaseError {
			source: Box::new(err),
		}
	}
}

// Convert from object_store errors
impl From<slatedb::object_store::Error> for StorageError {
	fn from(err: slatedb::object_store::Error) -> Self {
		Self::DatabaseError {
			source: Box::new(err),
		}
	}
}

impl From<url::ParseError> for StorageError {
	fn from(err: url::ParseError) -> Self {
		Self::ObjectStoreConfig {
			message: err.to_string(),
		}
	}
}

// Convert from UTF-8 conversion errors
impl From<std::str::Utf8Error> for StorageError {
	fn from(_err: std::str::Utf8Error) -> Self {
		Self::DecodeError {
			source: DecoderError::InvalidLength,
		}
	}
}

// Convert from array slice conversion errors
impl From<std::array::TryFromSliceError> for StorageError {
	fn from(_err: std::array::TryFromSliceError) -> Self {
		Self::DecodeError {
			source: DecoderError::InvalidLength,
		}
	}
}

// Convert from integer parsing errors
impl From<std::num::ParseIntError> for StorageError {
	fn from(_err: std::num::ParseIntError) -> Self {
		Self::DecodeError {
			source: DecoderError::InvalidType,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_error_codes() {
		assert_eq!(DecoderError::Empty.code(), "E0001");
		assert_eq!(DecoderError::InvalidType.code(), "E0002");
		assert_eq!(DecoderError::InvalidLength.code(), "E0003");
	}

	#[test]
	fn test_error_codes_unique() {
		let codes = [
			DecoderError::Empty.code(),
			DecoderError::InvalidType.code(),
			DecoderError::InvalidLength.code(),
		];
		let unique_codes: std::collections::HashSet<_> = codes.iter().collect();
		assert_eq!(
			codes.len(),
			unique_codes.len(),
			"Error codes must be unique"
		);
	}

	#[test]
	fn test_error_messages() {
		// Verify error messages still work correctly
		assert_eq!(DecoderError::Empty.to_string(), "Empty key, cannot decode");
		assert_eq!(DecoderError::InvalidType.to_string(), "Invalid type code");
		assert_eq!(
			DecoderError::InvalidLength.to_string(),
			"Invalid data length"
		);
	}

	#[test]
	fn test_storage_error_codes() {
		let db_err = StorageError::DatabaseError {
			source: "test error".into(),
		};
		assert_eq!(db_err.code(), "E1000");

		let decode_err = StorageError::from(DecoderError::Empty);
		assert_eq!(decode_err.code(), "E1002");

		let io_err = StorageError::from(std::io::Error::new(
			std::io::ErrorKind::NotFound,
			"not found",
		));
		assert_eq!(io_err.code(), "E1003");

		let inconsistency_err = StorageError::DataInconsistency {
			message: "test".into(),
		};
		assert_eq!(inconsistency_err.code(), "E1004");

		let object_store_err = StorageError::ObjectStoreConfig {
			message: "test".into(),
		};
		assert_eq!(object_store_err.code(), "E1005");

		let invalid_expiration = StorageError::InvalidExpiration {
			timestamp: u64::MAX,
			max: crate::expiration::MAX_EXPIRATION_TIMESTAMP_MS,
		};
		assert_eq!(invalid_expiration.code(), "E1007");
	}

	#[test]
	fn test_detailed_error_code() {
		// DecodeError should show nested code
		let decode_err = StorageError::from(DecoderError::Empty);
		assert_eq!(decode_err.detailed_code(), "E1002:E0001");

		let decode_err2 = StorageError::from(DecoderError::InvalidType);
		assert_eq!(decode_err2.detailed_code(), "E1002:E0002");

		// Other errors should just show their code
		let db_err = StorageError::DatabaseError {
			source: "test".into(),
		};
		assert_eq!(db_err.detailed_code(), "E1000");
	}

	#[test]
	fn test_storage_error_codes_unique() {
		let codes = [
			"E1000", "E1002", "E1003", "E1004", "E1005", "E1006", "E1007",
		];
		let unique_codes: std::collections::HashSet<_> = codes.iter().collect();
		assert_eq!(
			codes.len(),
			unique_codes.len(),
			"StorageError codes must be unique"
		);
	}

	#[test]
	fn test_from_decoder_error() {
		let decoder_err = DecoderError::Empty;
		let storage_err: StorageError = decoder_err.into();

		assert_eq!(storage_err.code(), "E1002");
		assert_eq!(storage_err.detailed_code(), "E1002:E0001");
	}

	#[test]
	fn test_from_io_error() {
		let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
		let storage_err: StorageError = io_err.into();

		assert_eq!(storage_err.code(), "E1003");
		assert!(storage_err.to_string().contains("access denied"));
	}
}
