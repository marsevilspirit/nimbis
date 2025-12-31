//! Error types for RESP parsing and encoding.

use thiserror::Error;

/// Main error type for RESP operations.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum RespError {
	/// Error during parsing
	#[error("Parse error: {0}")]
	Parse(#[from] ParseError),
}

/// Errors that can occur during RESP parsing.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum ParseError {
	/// Unexpected end of input while parsing
	#[error("Unexpected end of input")]
	UnexpectedEOF,

	/// Invalid type marker encountered
	#[error("Invalid type marker: {0}")]
	InvalidTypeMarker(char),

	/// Invalid format for the current type
	#[error("Invalid format: {0}")]
	InvalidFormat(String),

	/// Invalid integer value
	#[error("Invalid integer: {0}")]
	InvalidInteger(String),

	/// Invalid bulk string length
	#[error("Invalid bulk string length: {0}")]
	InvalidBulkStringLength(i64),

	/// Invalid array length
	#[error("Invalid array length: {0}")]
	InvalidArrayLength(i64),

	/// UTF-8 conversion error
	#[error("UTF-8 error: {0}")]
	Utf8Error(String),

	/// Invalid double value
	#[error("Invalid double: {0}")]
	InvalidDouble(String),
}

impl From<std::str::Utf8Error> for ParseError {
	fn from(e: std::str::Utf8Error) -> Self {
		ParseError::Utf8Error(e.to_string())
	}
}

impl From<std::num::ParseIntError> for ParseError {
	fn from(e: std::num::ParseIntError) -> Self {
		ParseError::InvalidInteger(e.to_string())
	}
}

impl From<std::num::ParseFloatError> for ParseError {
	fn from(e: std::num::ParseFloatError) -> Self {
		ParseError::InvalidDouble(e.to_string())
	}
}
