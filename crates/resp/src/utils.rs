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

/// Find the position of CRLF in a byte slice using SIMD-optimized memchr.
/// This is significantly faster than the naive windows() approach.
#[inline]
pub fn find_crlf(buf: &[u8]) -> Option<usize> {
	// Use memchr to quickly find '\r', then verify it's followed by '\n'
	let mut pos = 0;
	while let Some(cr_pos) = memchr::memchr(b'\r', &buf[pos..]) {
		let abs_pos = pos + cr_pos;
		// Check if there's a '\n' following the '\r'
		if abs_pos + 1 < buf.len() && buf[abs_pos + 1] == b'\n' {
			return Some(abs_pos);
		}
		// Move past this '\r' and continue searching
		pos = abs_pos + 1;
	}
	None
}

/// Peek a line from buffer (without CRLF), returning (line,
/// total_len_including_crlf) returns None if CRLF is not found
#[inline]
pub fn peek_line(buf: &[u8]) -> Option<(&[u8], usize)> {
	find_crlf(buf).map(|pos| (&buf[..pos], pos + 2))
}

/// Parses a RESP integer from a byte slice.
///
/// This is a high-performance, custom implementation that performs manual
/// parsing and overflow checking. It is designed to be faster than the standard
/// `std::str::from_utf8().parse()` approach by avoiding UTF-8 validation
/// and extra string allocations.
///
/// ### Input Format
/// - Optional leading `+` or `-` sign.
/// - One or more ASCII digits (`0`-`9`).
///
/// ### Errors
/// Returns a [`ParseError::InvalidInteger`] in the following cases:
/// - **Empty input**: If the input slice is empty or contains only a sign
///   character.
/// - **Invalid digits**: If any character is not an ASCII digit (after the
///   optional sign).
/// - **Overflow**: If the resulting value exceeds the range of a 64-bit signed
///   integer (`i64`).
///
/// ### Examples
/// ```
/// use resp::utils::parse_integer;
/// assert_eq!(parse_integer(b"123").unwrap(), 123);
/// assert_eq!(parse_integer(b"-456").unwrap(), -456);
/// assert_eq!(parse_integer(b"+789").unwrap(), 789);
/// ```
#[inline]
pub fn parse_integer(buf: &[u8]) -> Result<i64, ParseError> {
	let (digits, negative) = match buf {
		[b'-', rest @ ..] => (rest, true),
		[b'+', rest @ ..] => (rest, false),
		_ => (buf, false),
	};

	if digits.is_empty() {
		return Err(ParseError::InvalidInteger("no digits found".to_string()));
	}

	let mut result: i64 = 0;
	for &byte in digits {
		if !byte.is_ascii_digit() {
			return Err(ParseError::InvalidInteger(format!(
				"invalid digit: {}",
				byte as char
			)));
		}
		let digit = (byte - b'0') as i64;
		result = result
			.checked_mul(10)
			.and_then(|r| {
				if negative {
					r.checked_sub(digit)
				} else {
					r.checked_add(digit)
				}
			})
			.ok_or_else(|| ParseError::InvalidInteger("integer overflow".to_string()))?;
	}

	Ok(result)
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

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case(b"hello\r\n", Some(5))]
	#[case(b"hello", None)]
	#[case(b"\r\n", Some(0))]
	#[case(b"multiple\r\nlines\r\n", Some(8))]
	fn test_find_crlf(#[case] input: &[u8], #[case] expected: Option<usize>) {
		assert_eq!(find_crlf(input), expected);
	}

	#[rstest]
	#[case(b"123", Ok(123))]
	#[case(b"-456", Ok(-456))]
	#[case(b"+789", Ok(789))]
	#[case(b"0", Ok(0))]
	#[case(b"abc", Err(()))] // Use Err(()) as a placeholder for any error
	#[case(b"", Err(()))]
	#[case(b"-", Err(()))]
	#[case(b"9223372036854775808", Err(()))] // Overflow
	fn test_parse_integer(#[case] input: &[u8], #[case] expected: Result<i64, ()>) {
		let result = parse_integer(input);
		match expected {
			Ok(val) => assert_eq!(result.unwrap(), val),
			Err(_) => assert!(result.is_err()),
		}
	}

	#[rstest]
	#[case(b"3.14", 3.14)]
	#[case(b"-2.5", -2.5)]
	#[case(b"inf", f64::INFINITY)]
	#[case(b"-inf", f64::NEG_INFINITY)]
	fn test_parse_double(#[case] input: &[u8], #[case] expected: f64) {
		let result = parse_double(input).unwrap();
		if expected.is_infinite() {
			assert_eq!(result.is_infinite(), true);
			assert_eq!(result.is_sign_positive(), expected.is_sign_positive());
		} else {
			assert_eq!(result, expected);
		}
	}
}
