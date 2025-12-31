use std::collections::HashMap;
use std::collections::HashSet;

use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use resp::RespValue;
use thiserror::Error;

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
