//! RESP protocol encoder for serializing values to bytes.

use bytes::{BufMut, Bytes, BytesMut};
use std::collections::{HashMap, HashSet};

use crate::error::EncodeError;
use crate::types::RespValue;
use crate::utils::*;

/// Trait for encoding RESP values.
pub trait RespEncoder {
    /// Encode the value into a buffer.
    fn encode_to(&self, buf: &mut BytesMut) -> Result<(), EncodeError>;

    /// Encode the value and return the bytes.
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

/// Encode a simple string: `+OK\r\n`
#[inline]
fn encode_simple_string(buf: &mut BytesMut, s: &Bytes) {
    buf.put_u8(SIMPLE_STRING);
    buf.put_slice(s);
    buf.put_slice(CRLF);
}

/// Encode an error: `-ERR message\r\n`
#[inline]
fn encode_error(buf: &mut BytesMut, e: &Bytes) {
    buf.put_u8(ERROR);
    buf.put_slice(e);
    buf.put_slice(CRLF);
}

/// Encode an integer: `:1000\r\n`
#[inline]
fn encode_integer(buf: &mut BytesMut, i: i64) {
    buf.put_u8(INTEGER);
    buf.put_slice(i.to_string().as_bytes());
    buf.put_slice(CRLF);
}

/// Helper to encode a length value
#[inline]
fn encode_length(buf: &mut BytesMut, marker: u8, length: usize) {
    buf.put_u8(marker);
    buf.put_slice(length.to_string().as_bytes());
    buf.put_slice(CRLF);
}

/// Encode a bulk string: `$6\r\nfoobar\r\n`
#[inline]
fn encode_bulk_string(buf: &mut BytesMut, s: &Bytes) {
    encode_length(buf, BULK_STRING, s.len());
    buf.put_slice(s);
    buf.put_slice(CRLF);
}

/// Encode an array: `*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n`
fn encode_array(buf: &mut BytesMut, arr: &[RespValue]) -> Result<(), EncodeError> {
    encode_length(buf, ARRAY, arr.len());

    for value in arr {
        value.encode_to(buf)?;
    }

    Ok(())
}

/// Encode null (RESP3): `_\r\n` or (RESP2 as bulk string): `$-1\r\n`
#[inline]
fn encode_null(buf: &mut BytesMut) {
    // Use RESP3 null format
    buf.put_u8(NULL);
    buf.put_slice(CRLF);
}

/// Encode null as RESP2 bulk string: `$-1\r\n`
#[inline]
#[allow(dead_code)]
fn encode_null_bulk_string(buf: &mut BytesMut) {
    buf.put_u8(BULK_STRING);
    buf.put_slice(b"-1");
    buf.put_slice(CRLF);
}

/// Encode a boolean: `#t\r\n` or `#f\r\n`
#[inline]
fn encode_boolean(buf: &mut BytesMut, b: bool) {
    buf.put_u8(BOOLEAN);
    buf.put_u8(if b { b't' } else { b'f' });
    buf.put_slice(CRLF);
}

/// Encode a double: `,3.14\r\n`
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

/// Encode a big number: `(3492890328409238509324850943850943825024385\r\n`
#[inline]
fn encode_big_number(buf: &mut BytesMut, n: &Bytes) {
    buf.put_u8(BIG_NUMBER);
    buf.put_slice(n);
    buf.put_slice(CRLF);
}

/// Encode a bulk error: `!21\r\nSYNTAX invalid syntax\r\n`
#[inline]
fn encode_bulk_error(buf: &mut BytesMut, e: &Bytes) {
    encode_length(buf, BULK_ERROR, e.len());
    buf.put_slice(e);
    buf.put_slice(CRLF);
}

/// Encode a verbatim string: `=15\r\ntxt:Some string\r\n`
#[inline]
fn encode_verbatim_string(buf: &mut BytesMut, format: &Bytes, data: &Bytes) {
    let total_len = 4 + data.len(); // format (3) + ':' (1) + data
    encode_length(buf, VERBATIM_STRING, total_len);
    buf.put_slice(format);
    buf.put_u8(b':');
    buf.put_slice(data);
    buf.put_slice(CRLF);
}

/// Encode a map: `%2\r\n+first\r\n:1\r\n+second\r\n:2\r\n`
fn encode_map(buf: &mut BytesMut, map: &HashMap<RespValue, RespValue>) -> Result<(), EncodeError> {
    encode_length(buf, MAP, map.len());

    for (key, value) in map {
        key.encode_to(buf)?;
        value.encode_to(buf)?;
    }

    Ok(())
}

/// Encode a set: `~5\r\n+orange\r\n+apple\r\n...\r\n`
fn encode_set(buf: &mut BytesMut, set: &HashSet<RespValue>) -> Result<(), EncodeError> {
    encode_length(buf, SET, set.len());

    for value in set {
        value.encode_to(buf)?;
    }

    Ok(())
}

/// Encode a push: `>4\r\n+pubsub\r\n+message\r\n...\r\n`
fn encode_push(buf: &mut BytesMut, push: &[RespValue]) -> Result<(), EncodeError> {
    encode_length(buf, PUSH, push.len());

    for value in push {
        value.encode_to(buf)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RespParser;

    #[test]
    fn test_encode_simple_string() {
        let value = RespValue::SimpleString(Bytes::from("OK"));
        let encoded = value.encode().unwrap();
        assert_eq!(&encoded[..], b"+OK\r\n");
    }

    #[test]
    fn test_encode_error() {
        let value = RespValue::Error(Bytes::from("ERR unknown command"));
        let encoded = value.encode().unwrap();
        assert_eq!(&encoded[..], b"-ERR unknown command\r\n");
    }

    #[test]
    fn test_encode_integer() {
        let value = RespValue::Integer(1000);
        let encoded = value.encode().unwrap();
        assert_eq!(&encoded[..], b":1000\r\n");
    }

    #[test]
    fn test_encode_bulk_string() {
        let value = RespValue::BulkString(Bytes::from("foobar"));
        let encoded = value.encode().unwrap();
        assert_eq!(&encoded[..], b"$6\r\nfoobar\r\n");
    }

    #[test]
    fn test_encode_array() {
        let value = RespValue::Array(vec![
            RespValue::BulkString(Bytes::from("foo")),
            RespValue::BulkString(Bytes::from("bar")),
        ]);
        let encoded = value.encode().unwrap();
        assert_eq!(&encoded[..], b"*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n");
    }

    #[test]
    fn test_encode_null() {
        let value = RespValue::Null;
        let encoded = value.encode().unwrap();
        assert_eq!(&encoded[..], b"_\r\n");
    }

    #[test]
    fn test_encode_boolean() {
        let value = RespValue::Boolean(true);
        let encoded = value.encode().unwrap();
        assert_eq!(&encoded[..], b"#t\r\n");

        let value = RespValue::Boolean(false);
        let encoded = value.encode().unwrap();
        assert_eq!(&encoded[..], b"#f\r\n");
    }

    #[test]
    fn test_encode_double() {
        let value = RespValue::Double(3.14);
        let encoded = value.encode().unwrap();
        assert_eq!(&encoded[..], b",3.14\r\n");
    }

    #[test]
    fn test_roundtrip() {
        let original = RespValue::Array(vec![
            RespValue::BulkString(Bytes::from("SET")),
            RespValue::BulkString(Bytes::from("key")),
            RespValue::BulkString(Bytes::from("value")),
        ]);

        let encoded = original.encode().unwrap();
        let decoded = RespParser::parse_complete(&encoded).unwrap();

        assert_eq!(original, decoded);
    }
}
