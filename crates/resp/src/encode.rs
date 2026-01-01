use std::collections::HashMap;
use std::collections::HashSet;

use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use thiserror::Error;

use crate::RespValue;

/// Errors that can occur during RESP encoding.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum EncodeError {
	/// Value too large to encode
	#[error("Value too large: {0}")]
	ValueTooLarge(String),

	/// Invalid value for encoding
	#[error("Invalid value: {0}")]
	InvalidValue(String),
}

/// CRLF line ending
const CRLF: &[u8] = b"\r\n";

/// Type markers
const SIMPLE_STRING: u8 = b'+';
const ERROR: u8 = b'-';
const INTEGER: u8 = b':';
const BULK_STRING: u8 = b'$';
const ARRAY: u8 = b'*';
const NULL: u8 = b'_';
const BOOLEAN: u8 = b'#';
const DOUBLE: u8 = b',';
const BIG_NUMBER: u8 = b'(';
const BULK_ERROR: u8 = b'!';
const VERBATIM_STRING: u8 = b'=';
const MAP: u8 = b'%';
const SET: u8 = b'~';
const PUSH: u8 = b'>';

/// Trait for encoding RESP values.
pub trait RespEncoder {
	fn encode_to(&self, buf: &mut BytesMut) -> Result<(), EncodeError>;

	fn encode(&self) -> Result<Bytes, EncodeError> {
		let mut buf = BytesMut::new();
		self.encode_to(&mut buf)?;
		Ok(buf.freeze())
	}
}

impl RespEncoder for RespValue {
	fn encode_to(&self, buf: &mut BytesMut) -> Result<(), EncodeError> {
		match self {
			RespValue::SimpleString(s) => encode_simple_string(buf, s),
			RespValue::Error(e) => encode_error(buf, e),
			RespValue::Integer(i) => encode_integer(buf, *i),
			RespValue::BulkString(s) => encode_bulk_string(buf, s),
			RespValue::Array(arr) => encode_array(buf, arr)?,
			RespValue::Null => encode_null(buf),
			RespValue::Boolean(b) => encode_boolean(buf, *b),
			RespValue::Double(d) => encode_double(buf, *d),
			RespValue::BigNumber(n) => encode_big_number(buf, n),
			RespValue::BulkError(e) => encode_bulk_error(buf, e),
			RespValue::VerbatimString { format, data } => encode_verbatim_string(buf, format, data),
			RespValue::Map(m) => encode_map(buf, m)?,
			RespValue::Set(s) => encode_set(buf, s)?,
			RespValue::Push(p) => encode_push(buf, p)?,
		}
		Ok(())
	}
}

// Implementations (copied and adapted)

#[inline]
fn encode_simple_string(buf: &mut BytesMut, s: &Bytes) {
	buf.put_u8(SIMPLE_STRING);
	buf.put_slice(s);
	buf.put_slice(CRLF);
}

#[inline]
fn encode_error(buf: &mut BytesMut, e: &Bytes) {
	buf.put_u8(ERROR);
	buf.put_slice(e);
	buf.put_slice(CRLF);
}

#[inline]
fn encode_integer(buf: &mut BytesMut, i: i64) {
	buf.put_u8(INTEGER);
	buf.put_slice(i.to_string().as_bytes());
	buf.put_slice(CRLF);
}

#[inline]
fn encode_length(buf: &mut BytesMut, marker: u8, length: usize) {
	buf.put_u8(marker);
	buf.put_slice(length.to_string().as_bytes());
	buf.put_slice(CRLF);
}

#[inline]
fn encode_bulk_string(buf: &mut BytesMut, s: &Bytes) {
	encode_length(buf, BULK_STRING, s.len());
	buf.put_slice(s);
	buf.put_slice(CRLF);
}

fn encode_array(buf: &mut BytesMut, arr: &[RespValue]) -> Result<(), EncodeError> {
	encode_length(buf, ARRAY, arr.len());
	for value in arr {
		value.encode_to(buf)?;
	}
	Ok(())
}

#[inline]
fn encode_null(buf: &mut BytesMut) {
	buf.put_u8(NULL);
	buf.put_slice(CRLF);
}

#[inline]
fn encode_boolean(buf: &mut BytesMut, b: bool) {
	buf.put_u8(BOOLEAN);
	buf.put_u8(if b { b't' } else { b'f' });
	buf.put_slice(CRLF);
}

#[inline]
fn encode_double(buf: &mut BytesMut, d: f64) {
	buf.put_u8(DOUBLE);
	if d.is_infinite() {
		if d.is_sign_positive() {
			buf.put_slice(b"inf");
		} else {
			buf.put_slice(b"-inf");
		}
	} else {
		buf.put_slice(d.to_string().as_bytes());
	}
	buf.put_slice(CRLF);
}

#[inline]
fn encode_big_number(buf: &mut BytesMut, n: &Bytes) {
	buf.put_u8(BIG_NUMBER);
	buf.put_slice(n);
	buf.put_slice(CRLF);
}

#[inline]
fn encode_bulk_error(buf: &mut BytesMut, e: &Bytes) {
	encode_length(buf, BULK_ERROR, e.len());
	buf.put_slice(e);
	buf.put_slice(CRLF);
}

#[inline]
fn encode_verbatim_string(buf: &mut BytesMut, format: &Bytes, data: &Bytes) {
	let total_len = 4 + data.len();
	encode_length(buf, VERBATIM_STRING, total_len);
	buf.put_slice(format);
	buf.put_u8(b':');
	buf.put_slice(data);
	buf.put_slice(CRLF);
}

fn encode_map(buf: &mut BytesMut, map: &HashMap<RespValue, RespValue>) -> Result<(), EncodeError> {
	encode_length(buf, MAP, map.len());
	for (key, value) in map {
		key.encode_to(buf)?;
		value.encode_to(buf)?;
	}
	Ok(())
}

fn encode_set(buf: &mut BytesMut, set: &HashSet<RespValue>) -> Result<(), EncodeError> {
	encode_length(buf, SET, set.len());
	for value in set {
		value.encode_to(buf)?;
	}
	Ok(())
}

fn encode_push(buf: &mut BytesMut, push: &[RespValue]) -> Result<(), EncodeError> {
	encode_length(buf, PUSH, push.len());
	for value in push {
		value.encode_to(buf)?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[test]
	fn test_encode_simple_string() {
		let val = RespValue::SimpleString(Bytes::from_static(b"OK"));
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"+OK\r\n".as_slice());
	}

	#[test]
	fn test_encode_error() {
		let val = RespValue::Error(Bytes::from_static(b"ERR"));
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"-ERR\r\n".as_slice());
	}

	#[rstest]
	#[case(100, b":100\r\n")]
	#[case(-100, b":-100\r\n")]
	#[case(0, b":0\r\n")]
	fn test_encode_integer(#[case] input: i64, #[case] expected: &[u8]) {
		let val = RespValue::Integer(input);
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn test_encode_bulk_string() {
		let val = RespValue::BulkString(Bytes::from_static(b"hello"));
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"$5\r\nhello\r\n".as_slice());
	}

	#[test]
	fn test_encode_bulk_string_empty() {
		let val = RespValue::BulkString(Bytes::new());
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"$0\r\n\r\n".as_slice());
	}

	#[test]
	fn test_encode_array() {
		let val = RespValue::Array(vec![
			RespValue::SimpleString(Bytes::from_static(b"hello")),
			RespValue::Integer(42),
		]);
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"*2\r\n+hello\r\n:42\r\n".as_slice());
	}

	#[test]
	fn test_encode_array_empty() {
		let val = RespValue::Array(vec![]);
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"*0\r\n".as_slice());
	}

	#[test]
	fn test_encode_null() {
		let val = RespValue::Null;
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"_\r\n".as_slice());
	}

	#[rstest]
	#[case(true, b"#t\r\n")]
	#[case(false, b"#f\r\n")]
	fn test_encode_boolean(#[case] input: bool, #[case] expected: &[u8]) {
		let val = RespValue::Boolean(input);
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, expected);
	}

	#[rstest]
	#[case(3.14, b",3.14\r\n")]
	#[case(10.0, b",10\r\n")]
	#[case(f64::INFINITY, b",inf\r\n")]
	#[case(f64::NEG_INFINITY, b",-inf\r\n")]
	fn test_encode_double(#[case] input: f64, #[case] expected: &[u8]) {
		let val = RespValue::Double(input);
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, expected);
	}

	#[test]
	fn test_encode_big_number() {
		let val = RespValue::BigNumber(Bytes::from_static(b"12345678901234567890"));
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"(12345678901234567890\r\n".as_slice());
	}

	#[test]
	fn test_encode_bulk_error() {
		let val = RespValue::BulkError(Bytes::from_static(b"ERR"));
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"!3\r\nERR\r\n".as_slice());
	}

	#[test]
	fn test_encode_verbatim_string() {
		let val = RespValue::VerbatimString {
			format: Bytes::from_static(b"txt"),
			data: Bytes::from_static(b"msg"),
		};
		let encoded = val.encode().unwrap();
		// length = 4 (format + :) + 3 (data) = 7
		assert_eq!(encoded, b"=7\r\ntxt:msg\r\n".as_slice());
	}

	#[test]
	fn test_encode_map() {
		let mut map = HashMap::new();
		map.insert(
			RespValue::SimpleString(Bytes::from_static(b"k1")),
			RespValue::Integer(1),
		);
		let val = RespValue::Map(map);
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"%1\r\n+k1\r\n:1\r\n".as_slice());
	}

	#[test]
	fn test_encode_set() {
		let mut set = HashSet::new();
		set.insert(RespValue::SimpleString(Bytes::from_static(b"v1")));
		let val = RespValue::Set(set);
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b"~1\r\n+v1\r\n".as_slice());
	}

	#[test]
	fn test_encode_push() {
		let val = RespValue::Push(vec![
			RespValue::SimpleString(Bytes::from_static(b"pubsub")),
			RespValue::SimpleString(Bytes::from_static(b"message")),
		]);
		let encoded = val.encode().unwrap();
		assert_eq!(encoded, b">2\r\n+pubsub\r\n+message\r\n".as_slice());
	}
}
