//! High-performance RESP protocol parser with zero-copy optimizations.

use std::collections::HashMap;
use std::collections::HashSet;

use bytes::Buf;
use bytes::Bytes;
use bytes::BytesMut;

use crate::error::ParseError;
use crate::error::RespError;
use crate::types::RespValue;
use crate::utils::*;

/// Result of a parsing attempt.
#[derive(Debug)]
pub enum RespParseResult {
	/// A complete RESP value was parsed.
	Complete(RespValue),
	/// The buffer does not contain enough data to parse a complete value.
	Incomplete,
	/// An error occurred during parsing.
	Error(RespError),
}

/// A stateful RESP parser that supports streaming.
pub struct RespParser {
	frames: Vec<Frame>,
}

#[derive(Debug)]
enum Frame {
	Root,
	Array {
		expected: usize,
		elements: Vec<RespValue>,
	},
	Map {
		expected: usize,
		elements: HashMap<RespValue, RespValue>,
		key: Option<RespValue>, // Temporary storage for key
	},
	Set {
		expected: usize,
		elements: HashSet<RespValue>,
	},
	Push {
		expected: usize,
		elements: Vec<RespValue>,
	},
}

impl Default for RespParser {
	fn default() -> Self {
		Self::new()
	}
}

// Helper enum for parse_step
enum ParsedItem {
	Value(RespValue),
	FramePushed,
}

impl RespParser {
	pub fn new() -> Self {
		Self { frames: Vec::new() }
	}

	/// Parse a RESP value from a mutable BytesMut buffer.
	///
	/// If successful, consumes the parsed bytes and returns
	/// `RespParseResult::Complete(value)`. If incomplete, returns
	/// `RespParseResult::Incomplete`. If an error occurs, returns
	/// `RespParseResult::Error(error)`.
	pub fn parse(&mut self, buf: &mut BytesMut) -> RespParseResult {
		if self.frames.is_empty() {
			self.frames.push(Frame::Root);
		}

		loop {
			// 1. Try to parse next item
			match self.parse_step(buf) {
				Ok(Some(ParsedItem::FramePushed)) => {
					continue;
				}
				Ok(Some(ParsedItem::Value(val))) => {
					// We got a value, inject it into current frame
					match self.handle_parsed_value(val) {
						Ok(Some(final_value)) => return RespParseResult::Complete(final_value),
						Ok(None) => continue,
						Err(e) => return RespParseResult::Error(RespError::Parse(e)),
					}
				}
				Ok(None) => return RespParseResult::Incomplete,
				Err(e) => return RespParseResult::Error(RespError::Parse(e)),
			}
		}
	}

	// Helper to handle a successfully obtained value (either primitive or a
	// finished collection)
	fn handle_parsed_value(&mut self, value: RespValue) -> Result<Option<RespValue>, ParseError> {
		// This function injects the value into the current top frame.
		// Returns `Some(RespValue)` if the ROOT value is completed.
		// Returns `None` if we successfully absorbed the value but need more.

		let current_frame_idx = self
			.frames
			.len()
			.checked_sub(1)
			.ok_or_else(|| ParseError::InvalidFormat("Internal stack error".into()))?;

		match &mut self.frames[current_frame_idx] {
			Frame::Root => {
				// We parsed a full value at root!
				// We must pop the root frame to reset parser state for next command,
				// or expects the caller to reuse or `parse` adds it back.
				// Since `parse` checks `if self.frames.is_empty()`, popping is safe.
				self.frames.pop();
				Ok(Some(value))
			}
			Frame::Array { expected, elements } => {
				elements.push(value);
				*expected -= 1;
				if *expected == 0 {
					let arr = std::mem::take(elements);
					self.frames.pop();
					self.handle_parsed_value(arr.into())
				} else {
					Ok(None)
				}
			}
			Frame::Map {
				expected,
				elements,
				key,
			} => {
				if let Some(k) = key.take() {
					elements.insert(k, value);
					*expected -= 1;
				} else {
					*key = Some(value);
				}

				if *expected == 0 {
					let map = std::mem::take(elements);
					self.frames.pop();
					self.handle_parsed_value(RespValue::Map(map))
				} else {
					Ok(None)
				}
			}
			Frame::Set { expected, elements } => {
				elements.insert(value);
				*expected -= 1;
				if *expected == 0 {
					let set = std::mem::take(elements);
					self.frames.pop();
					self.handle_parsed_value(RespValue::Set(set))
				} else {
					Ok(None)
				}
			}
			Frame::Push { expected, elements } => {
				elements.push(value);
				*expected -= 1;
				if *expected == 0 {
					let arr = std::mem::take(elements);
					self.frames.pop();
					self.handle_parsed_value(RespValue::Push(arr))
				} else {
					Ok(None)
				}
			}
		}
	}

	/// Tries to parse the next token.
	/// If it's a primitive, returns `Ok(Some(Parsed::Value(v)))`.
	/// If it's a collection start, pushes frame and returns
	/// `Ok(Some(Parsed::FramePushed))`. If incomplete, returns `Ok(None)`.
	fn parse_step(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if buf.is_empty() {
			return Ok(None);
		}

		// Peek type marker
		let type_marker = buf[0];

		match type_marker {
			SIMPLE_STRING => self.parse_simple_string(buf),
			ERROR => self.parse_error(buf),
			INTEGER => self.parse_integer(buf),
			BULK_STRING => self.parse_bulk_string(buf),
			BOOLEAN => self.parse_boolean(buf),
			DOUBLE => self.parse_double(buf),
			BIG_NUMBER => self.parse_big_number(buf),
			BULK_ERROR => self.parse_bulk_error(buf),
			VERBATIM_STRING => self.parse_verbatim_string(buf),
			NULL => self.parse_null(buf),

			// Collections
			ARRAY => self.start_array(buf),
			MAP => self.start_map(buf),
			SET => self.start_set(buf),
			PUSH => self.start_push(buf),

			_ => {
				if Self::is_possible_inline_type_marker(type_marker) {
					self.parse_inline_command(buf)
				} else {
					Err(ParseError::InvalidTypeMarker(type_marker as char))
				}
			}
		}
	}

	/// Checks if a byte is a valid start for an inline command (printable ASCII
	/// or space). Also allows '\r' to support empty lines (CRLF).
	#[inline]
	fn is_possible_inline_type_marker(c: u8) -> bool {
		c.is_ascii_graphic() || c == b' ' || c == b'\r'
	}

	fn parse_inline_command(
		&mut self,
		buf: &mut BytesMut,
	) -> Result<Option<ParsedItem>, ParseError> {
		loop {
			if let Some((line, total_len)) = peek_line(buf) {
				// Add a length check to prevent DoS from very long inline commands.
				const MAX_INLINE_COMMAND_LEN: usize = 65536; // 64KB
				if line.len() > MAX_INLINE_COMMAND_LEN {
					return Err(ParseError::InvalidFormat(format!(
						"Inline command exceeds maximum length of {} bytes",
						MAX_INLINE_COMMAND_LEN
					)));
				}

				// Check if the first byte is a printable ASCII character.
				// This prevents interpreting binary data or weird control characters as inline
				// commands. Allowed: 0x21-0x7E (printable) and 0x20 (space).
				if !line.is_empty() {
					let first_char = line[0];
					if !(first_char.is_ascii_graphic() || first_char == b' ') {
						// This should have been caught by parse_step ->
						// is_possible_inline_type_marker But we keep it as a secondary safety
						// check or for direct calls
						return Err(ParseError::InvalidFormat(format!(
							"Inline command must start with a printable ASCII character, found: 0x{:02X}",
							first_char
						)));
					}
				}

				// We need valid UTF-8 to split by whitespace easily.
				// Best effort conversion.
				let s = std::str::from_utf8(line).map_err(|e| {
					ParseError::InvalidFormat(format!("Invalid inline command: {}", e))
				})?;

				let parts: Vec<&str> = s.split_whitespace().collect();
				if parts.is_empty() {
					// Empty line or whitespace only? Just consume and continue looking.
					// Redis ignores empty newlines.
					buf.advance(total_len);
					continue;
				}

				// This is an inline command.
				// Format: "CMD arg1 arg2 ...\r\n"
				// Split by whitespace and return as Array of BulkStrings.
				//
				// NOTE: This simple parsing does NOT support quoted strings with spaces.
				// E.g. `SET key "value with spaces"` will be parsed as ["SET", "key",
				// "\"value", "with", "spaces\""]. This is consistent with Redis inline
				// protocol which is meant for simple telnet usage.

				// NOTE: We use Bytes::copy_from_slice which allocates new memory for each
				// argument. Since split_whitespace() returns slices that may not be
				// contiguous in the original buffer (due to skipped whitespace), zero-copy
				// slicing is complex here. Given inline commands are typically short
				// control commands, this copy is acceptable.
				let args: Vec<RespValue> = parts
					.into_iter()
					.map(|p| RespValue::BulkString(Bytes::copy_from_slice(p.as_bytes())))
					.collect();

				buf.advance(total_len);
				return Ok(Some(ParsedItem::Value(RespValue::Array(args))));
			} else {
				return Ok(None);
			}
		}
	}

	fn parse_simple_string(
		&mut self,
		buf: &mut BytesMut,
	) -> Result<Option<ParsedItem>, ParseError> {
		// Use peek_line logic
		// buf[0] is '+'
		if let Some((line, total_len)) = peek_line(&buf[1..]) {
			let value = Bytes::copy_from_slice(line);
			buf.advance(1 + total_len);
			Ok(Some(ParsedItem::Value(RespValue::SimpleString(value))))
		} else {
			Ok(None)
		}
	}

	fn parse_error(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if let Some((line, total_len)) = peek_line(&buf[1..]) {
			let value = Bytes::copy_from_slice(line);
			buf.advance(1 + total_len);
			Ok(Some(ParsedItem::Value(RespValue::Error(value))))
		} else {
			Ok(None)
		}
	}

	fn parse_integer(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if let Some((line, total_len)) = peek_line(&buf[1..]) {
			let num = crate::utils::parse_integer(line)?;
			buf.advance(1 + total_len);
			Ok(Some(ParsedItem::Value(RespValue::Integer(num))))
		} else {
			Ok(None)
		}
	}

	fn parse_boolean(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if let Some((line, total_len)) = peek_line(&buf[1..]) {
			let value = match line {
				b"t" => true,
				b"f" => false,
				_ => {
					return Err(ParseError::InvalidFormat(
						"Boolean must be 't' or 'f'".to_string(),
					));
				}
			};
			buf.advance(1 + total_len);
			Ok(Some(ParsedItem::Value(RespValue::Boolean(value))))
		} else {
			Ok(None)
		}
	}

	fn parse_double(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if let Some((line, total_len)) = peek_line(&buf[1..]) {
			let value = crate::utils::parse_double(line)?;
			buf.advance(1 + total_len);
			Ok(Some(ParsedItem::Value(RespValue::Double(value))))
		} else {
			Ok(None)
		}
	}

	fn parse_big_number(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if let Some((line, total_len)) = peek_line(&buf[1..]) {
			let value = Bytes::copy_from_slice(line);
			buf.advance(1 + total_len);
			Ok(Some(ParsedItem::Value(RespValue::BigNumber(value))))
		} else {
			Ok(None)
		}
	}

	fn parse_null(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if let Some((_, total_len)) = peek_line(&buf[1..]) {
			buf.advance(1 + total_len);
			Ok(Some(ParsedItem::Value(RespValue::Null)))
		} else {
			Ok(None)
		}
	}

	fn parse_bulk_string(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		// $6\r\nfoobar\r\n
		if let Some((line, len_consumed)) = peek_line(&buf[1..]) {
			let length = crate::utils::parse_integer(line)?;

			if length == -1 {
				buf.advance(1 + len_consumed);
				return Ok(Some(ParsedItem::Value(RespValue::Null)));
			}
			if length < -1 {
				return Err(ParseError::InvalidBulkStringLength(length));
			}

			let length = length as usize;
			let total_needed = 1 + len_consumed + length + 2; // +2 for CRLF

			if buf.len() < total_needed {
				return Ok(None);
			}

			// All good, consume
			buf.advance(1 + len_consumed);
			let data = buf.split_to(length).freeze();
			if buf.len() < 2 || &buf[0..2] != CRLF {
				return Err(ParseError::InvalidFormat(
					"Missing CRLF after bulk string".to_string(),
				));
			}
			buf.advance(2);

			Ok(Some(ParsedItem::Value(RespValue::BulkString(data))))
		} else {
			Ok(None)
		}
	}

	fn parse_bulk_error(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		// Similar to bulk string
		if let Some((line, len_consumed)) = peek_line(&buf[1..]) {
			let length = crate::utils::parse_integer(line)?;

			if length < 0 {
				return Err(ParseError::InvalidBulkStringLength(length));
			}

			let length = length as usize;
			let total_needed = 1 + len_consumed + length + 2;

			if buf.len() < total_needed {
				return Ok(None);
			}

			buf.advance(1 + len_consumed);
			let data = buf.split_to(length).freeze();
			if buf.len() < 2 || &buf[0..2] != CRLF {
				return Err(ParseError::InvalidFormat(
					"Missing CRLF after bulk error".to_string(),
				));
			}
			buf.advance(2);

			Ok(Some(ParsedItem::Value(RespValue::BulkError(data))))
		} else {
			Ok(None)
		}
	}

	fn parse_verbatim_string(
		&mut self,
		buf: &mut BytesMut,
	) -> Result<Option<ParsedItem>, ParseError> {
		// =15\r\ntxt:Some string\r\n
		if let Some((line, len_consumed)) = peek_line(&buf[1..]) {
			let length = crate::utils::parse_integer(line)?;
			if length < 0 {
				return Err(ParseError::InvalidBulkStringLength(length));
			}
			let length = length as usize;
			let total_needed = 1 + len_consumed + length + 2;

			if buf.len() < total_needed {
				return Ok(None);
			}

			buf.advance(1 + len_consumed);
			let data = buf.split_to(length).freeze();
			if buf.len() < 2 || &buf[0..2] != CRLF {
				return Err(ParseError::InvalidFormat(
					"Missing CRLF after verbatim string".to_string(),
				));
			}
			buf.advance(2);

			if data.len() < 4 || data[3] != b':' {
				return Err(ParseError::InvalidFormat(
					"Verbatim string must have format prefix".to_string(),
				));
			}

			let format = data.slice(0..3);
			let content = data.slice(4..);

			Ok(Some(ParsedItem::Value(RespValue::VerbatimString {
				format,
				data: content,
			})))
		} else {
			Ok(None)
		}
	}

	// Collections start

	fn start_array(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if let Some((line, total_len)) = peek_line(&buf[1..]) {
			let length = crate::utils::parse_integer(line)?;
			buf.advance(1 + total_len);

			if length == -1 {
				return Ok(Some(ParsedItem::Value(RespValue::Null)));
			}
			if length < -1 {
				return Err(ParseError::InvalidArrayLength(length));
			}

			let length = length as usize;
			if length == 0 {
				return Ok(Some(ParsedItem::Value(RespValue::Array(Vec::new()))));
			}

			self.frames.push(Frame::Array {
				expected: length,
				elements: Vec::with_capacity(length),
			});
			Ok(Some(ParsedItem::FramePushed))
		} else {
			Ok(None)
		}
	}

	fn start_set(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if let Some((line, total_len)) = peek_line(&buf[1..]) {
			let length = crate::utils::parse_integer(line)?;
			buf.advance(1 + total_len);

			if length == -1 {
				return Ok(Some(ParsedItem::Value(RespValue::Null)));
			}
			if length < -1 {
				return Err(ParseError::InvalidArrayLength(length)); // Reuse error
			}

			let length = length as usize;
			if length == 0 {
				return Ok(Some(ParsedItem::Value(RespValue::Set(HashSet::new()))));
			}

			self.frames.push(Frame::Set {
				expected: length,
				elements: HashSet::with_capacity(length),
			});
			Ok(Some(ParsedItem::FramePushed))
		} else {
			Ok(None)
		}
	}

	fn start_map(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if let Some((line, total_len)) = peek_line(&buf[1..]) {
			let length = crate::utils::parse_integer(line)?;
			buf.advance(1 + total_len);

			if length == -1 {
				return Ok(Some(ParsedItem::Value(RespValue::Null)));
			}
			if length < -1 {
				return Err(ParseError::InvalidArrayLength(length));
			}

			let length = length as usize;
			if length == 0 {
				return Ok(Some(ParsedItem::Value(RespValue::Map(HashMap::new()))));
			}

			self.frames.push(Frame::Map {
				expected: length,
				elements: HashMap::with_capacity(length),
				key: None,
			});
			Ok(Some(ParsedItem::FramePushed))
		} else {
			Ok(None)
		}
	}

	fn start_push(&mut self, buf: &mut BytesMut) -> Result<Option<ParsedItem>, ParseError> {
		if let Some((line, total_len)) = peek_line(&buf[1..]) {
			let length = crate::utils::parse_integer(line)?;
			buf.advance(1 + total_len);

			if length == -1 {
				return Ok(Some(ParsedItem::Value(RespValue::Null)));
			}
			if length < -1 {
				return Err(ParseError::InvalidArrayLength(length));
			}

			let length = length as usize;
			if length == 0 {
				return Ok(Some(ParsedItem::Value(RespValue::Push(Vec::new()))));
			}

			self.frames.push(Frame::Push {
				expected: length,
				elements: Vec::with_capacity(length),
			});
			Ok(Some(ParsedItem::FramePushed))
		} else {
			Ok(None)
		}
	}
}

/// Convenience function for one-off parsing.
/// This will create a temporary parser and try to parse one value.
/// If streaming is needed, use `RespParser` directly.
pub fn parse(buf: &mut BytesMut) -> Result<RespValue, ParseError> {
	let mut parser = RespParser::new();
	match parser.parse(buf) {
		RespParseResult::Complete(val) => Ok(val),
		RespParseResult::Incomplete => Err(ParseError::UnexpectedEOF),
		RespParseResult::Error(RespError::Parse(e)) => Err(e),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_parse_simple_string() {
		let mut buf = BytesMut::from(&b"+OK\r\n"[..]);
		let value = parse(&mut buf).unwrap();
		assert_eq!(value, RespValue::SimpleString(Bytes::from("OK")));
	}

	#[test]
	fn test_parse_error() {
		let mut buf = BytesMut::from(&b"-ERR unknown command\r\n"[..]);
		let value = parse(&mut buf).unwrap();
		assert_eq!(value, RespValue::Error(Bytes::from("ERR unknown command")));
	}

	#[test]
	fn test_parse_integer() {
		let mut buf = BytesMut::from(&b":1000\r\n"[..]);
		let value = parse(&mut buf).unwrap();
		assert_eq!(value, RespValue::Integer(1000));
	}

	#[test]
	fn test_parse_bulk_string() {
		let mut buf = BytesMut::from(&b"$6\r\nfoobar\r\n"[..]);
		let value = parse(&mut buf).unwrap();
		assert_eq!(value, RespValue::BulkString(Bytes::from("foobar")));
	}

	#[test]
	fn test_parse_array() {
		let mut buf = BytesMut::from(&b"*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n"[..]);
		let value = parse(&mut buf).unwrap();

		if let RespValue::Array(arr) = value {
			assert_eq!(arr.len(), 2);
			assert_eq!(arr[0], RespValue::BulkString(Bytes::from("foo")));
			assert_eq!(arr[1], RespValue::BulkString(Bytes::from("bar")));
		} else {
			panic!("Expected Array, got {:?}", value);
		}
	}

	use rstest::rstest;

	#[rstest]
	#[case(b"PING\r\n", vec!["PING"])]
	#[case(b"SET key val\r\n", vec!["SET", "key", "val"])]
	#[case(b"  GET    key  \r\n", vec!["GET", "key"])]
	#[case(b"\r\nPING\r\n", vec!["PING"])] // Empty line skipped
	#[case(b" \r\nPING\r\n", vec!["PING"])] // Whitespace only line skipped
	#[case(b" PING\r\n", vec!["PING"])] // Starts with space
	#[case(b"GET\tkey\r\n", vec!["GET", "key"])] // Tab separator
	#[case(b"SET key \"val with spaces\"\r\n", vec!["SET", "key", "\"val", "with", "spaces\""])] // Quotes not handled
	fn test_parse_inline_command_valid(#[case] input: &[u8], #[case] expected: Vec<&str>) {
		let mut buf = BytesMut::from(input);
		let value = parse(&mut buf).unwrap();

		if expected.is_empty() {
			if let RespValue::Array(arr) = value {
				assert!(arr.is_empty(), "Expected empty array, got {:?}", arr);
			} else {
				panic!("Expected Array, got {:?}", value);
			}
		} else {
			if let RespValue::Array(arr) = value {
				assert_eq!(arr.len(), expected.len());
				for (i, expected_str) in expected.iter().enumerate() {
					assert_eq!(
						arr[i],
						RespValue::BulkString(Bytes::copy_from_slice(expected_str.as_bytes()))
					);
				}
			} else {
				panic!("Expected Array, got {:?}", value);
			}
		}
	}

	#[rstest]
	#[case(b"PING \x80\r\n", "Invalid inline command")] // Invalid UTF-8
	#[case(b"\x01PING\r\n", "Invalid type marker")] // Control char start -> InvalidTypeMarker
	#[case(b"\x7FPING\r\n", "Invalid type marker")] // Non-printable start -> InvalidTypeMarker
	fn test_parse_inline_command_invalid(#[case] input: &[u8], #[case] error_msg_part: &str) {
		let mut buf = BytesMut::from(input);
		let result = parse(&mut buf);
		// Check string representation because error types differ (InvalidFormat vs
		// InvalidTypeMarker)
		assert!(
			result
				.as_ref()
				.unwrap_err()
				.to_string()
				.contains(error_msg_part),
			"Expected error containing '{}', got {:?}",
			error_msg_part,
			result
		);
	}

	#[test]
	fn test_parse_inline_command_too_long() {
		// 64KB + 1 byte
		let mut big_cmd = Vec::new();
		big_cmd.extend(std::iter::repeat(b'a').take(65537));
		big_cmd.extend_from_slice(b"\r\n");
		let mut buf = BytesMut::from(&big_cmd[..]);

		let result = parse(&mut buf);
		assert!(
			matches!(result, Err(ParseError::InvalidFormat(msg)) if msg.contains("exceeds maximum length"))
		);
	}
}
