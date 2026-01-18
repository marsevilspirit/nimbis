use thiserror::Error;

use crate::data_type::DataType;

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

	/// Type checking error - operation against wrong data type
	#[error(
		"WRONGTYPE Operation against a key holding the wrong kind of value (expected: {expected:?}, actual: {actual:?})"
	)]
	WrongType {
		expected: Option<DataType>,
		actual: DataType,
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
}

impl StorageError {
	/// Returns the error code for this error variant
	pub fn code(&self) -> &'static str {
		match self {
			Self::DatabaseError { .. } => "E1000",
			Self::WrongType { .. } => "E1001",
			Self::DecodeError { .. } => "E1002",
			Self::IoError { .. } => "E1003",
			Self::DataInconsistency { .. } => "E1004",
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

	/// Helper to create a WrongType error with expected type
	pub fn wrong_type(expected: DataType, actual: DataType) -> Self {
		Self::WrongType {
			expected: Some(expected),
			actual,
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
		let codes = vec![
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

		let wrong_type_err = StorageError::wrong_type(DataType::String, DataType::Hash);
		assert_eq!(wrong_type_err.code(), "E1001");

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
		let codes = vec!["E1000", "E1001", "E1002", "E1003", "E1004"];
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
