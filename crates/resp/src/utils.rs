//! Utility functions and constants for RESP protocol.

use crate::error::ParseError;

/// CRLF line ending
pub const CRLF: &[u8] = b"\r\n";

/// Type markers for RESP2
pub const SIMPLE_STRING: u8 = b'+';
pub const ERROR: u8 = b'-';
pub const INTEGER: u8 = b':';
pub const BULK_STRING: u8 = b'$';
pub const ARRAY: u8 = b'*';

/// Type markers for RESP3
pub const NULL: u8 = b'_';
pub const BOOLEAN: u8 = b'#';
pub const DOUBLE: u8 = b',';
pub const BIG_NUMBER: u8 = b'(';
pub const BULK_ERROR: u8 = b'!';
pub const VERBATIM_STRING: u8 = b'=';
pub const MAP: u8 = b'%';
pub const SET: u8 = b'~';
pub const PUSH: u8 = b'>';

/// Find the position of CRLF in a byte slice
#[inline]
pub fn find_crlf(buf: &[u8]) -> Option<usize> {
	buf.windows(2).position(|window| window == CRLF)
}

/// Extract a line from buffer (without CRLF)
#[inline]
pub fn extract_line(buf: &[u8]) -> Result<(&[u8], usize), ParseError> {
	match find_crlf(buf) {
		Some(pos) => Ok((&buf[..pos], pos + 2)),
		None => Err(ParseError::UnexpectedEOF),
	}
}

/// Parse an integer from a byte slice
#[inline]
pub fn parse_integer(buf: &[u8]) -> Result<i64, ParseError> {
	let s = std::str::from_utf8(buf)?;
	s.parse::<i64>()
		.map_err(|e| ParseError::InvalidInteger(e.to_string()))
}

/// Parse a double from a byte slice
#[inline]
pub fn parse_double(buf: &[u8]) -> Result<f64, ParseError> {
	let s = std::str::from_utf8(buf)?;

	// Handle special values
	match s {
		"inf" => Ok(f64::INFINITY),
		"-inf" => Ok(f64::NEG_INFINITY),
		_ => s
			.parse::<f64>()
			.map_err(|e| ParseError::InvalidDouble(e.to_string())),
	}
}

/// Check if a type marker is valid
#[inline]
#[allow(dead_code)]
pub fn is_valid_type_marker(marker: u8) -> bool {
	matches!(
		marker,
		SIMPLE_STRING
			| ERROR | INTEGER
			| BULK_STRING
			| ARRAY | NULL
			| BOOLEAN | DOUBLE
			| BIG_NUMBER
			| BULK_ERROR
			| VERBATIM_STRING
			| MAP | SET
			| PUSH
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_find_crlf() {
		assert_eq!(find_crlf(b"hello\r\n"), Some(5));
		assert_eq!(find_crlf(b"hello"), None);
		assert_eq!(find_crlf(b"\r\n"), Some(0));
	}

	#[test]
	fn test_extract_line() {
		let (line, consumed) = extract_line(b"hello\r\nworld").unwrap();
		assert_eq!(line, b"hello");
		assert_eq!(consumed, 7);
	}

	#[test]
	fn test_parse_integer() {
		assert_eq!(parse_integer(b"123").unwrap(), 123);
		assert_eq!(parse_integer(b"-456").unwrap(), -456);
		assert!(parse_integer(b"abc").is_err());
	}

	#[test]
	fn test_parse_double() {
		assert_eq!(parse_double(b"3.14").unwrap(), 3.14);
		assert_eq!(parse_double(b"-2.5").unwrap(), -2.5);
		assert_eq!(parse_double(b"inf").unwrap(), f64::INFINITY);
		assert_eq!(parse_double(b"-inf").unwrap(), f64::NEG_INFINITY);
	}
}
