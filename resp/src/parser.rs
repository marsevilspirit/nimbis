//! High-performance RESP protocol parser with zero-copy optimizations.

use bytes::{Buf, Bytes, BytesMut};
use std::collections::{HashMap, HashSet};

use crate::error::ParseError;
use crate::types::RespValue;
use crate::utils::*;

/// RESP protocol parser with incremental parsing support.
///
/// The parser can handle incomplete data and resume parsing when more data arrives.
pub struct RespParser {
    // State for incremental parsing could be added here if needed
}

impl RespParser {
    /// Create a new parser instance.
    pub fn new() -> Self {
        Self {}
    }

    /// Parse a RESP value from a buffer.
    ///
    /// Returns `Ok(Some(value))` if a complete value was parsed,
    /// `Ok(None)` if more data is needed, or `Err` on parse error.
    ///
    /// On success, the parsed bytes are consumed from the buffer.
    pub fn parse(&mut self, buf: &mut BytesMut) -> Result<Option<RespValue>, ParseError> {
        if buf.is_empty() {
            return Ok(None);
        }

        // Try to parse, if incomplete return None
        match self.parse_value(buf) {
            Ok(value) => Ok(Some(value)),
            Err(ParseError::Incomplete) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Parse a complete RESP value from a byte slice.
    ///
    /// This is a convenience method for parsing complete messages.
    pub fn parse_complete(buf: &[u8]) -> Result<RespValue, ParseError> {
        let mut parser = Self::new();
        let mut bytes = BytesMut::from(buf);
        parser.parse_value(&mut bytes)
    }

    /// Internal parsing function that consumes bytes from the buffer.
    fn parse_value(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        if buf.is_empty() {
            return Err(ParseError::Incomplete);
        }

        let type_marker = buf[0];

        match type_marker {
            SIMPLE_STRING => self.parse_simple_string(buf),
            ERROR => self.parse_error(buf),
            INTEGER => self.parse_integer(buf),
            BULK_STRING => self.parse_bulk_string(buf),
            ARRAY => self.parse_array(buf),
            NULL => self.parse_null(buf),
            BOOLEAN => self.parse_boolean(buf),
            DOUBLE => self.parse_double(buf),
            BIG_NUMBER => self.parse_big_number(buf),
            BULK_ERROR => self.parse_bulk_error(buf),
            VERBATIM_STRING => self.parse_verbatim_string(buf),
            MAP => self.parse_map(buf),
            SET => self.parse_set(buf),
            PUSH => self.parse_push(buf),
            _ => Err(ParseError::InvalidTypeMarker(type_marker as char)),
        }
    }

    /// Parse a simple string: `+OK\r\n`
    fn parse_simple_string(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '+'
        let (line, consumed) = extract_line(buf)?;
        let value = Bytes::copy_from_slice(line);
        buf.advance(consumed);
        Ok(RespValue::SimpleString(value))
    }

    /// Parse an error: `-ERR message\r\n`
    fn parse_error(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '-'
        let (line, consumed) = extract_line(buf)?;
        let value = Bytes::copy_from_slice(line);
        buf.advance(consumed);
        Ok(RespValue::Error(value))
    }

    /// Parse an integer: `:1000\r\n`
    fn parse_integer(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip ':'
        let (line, consumed) = extract_line(buf)?;
        let num = parse_integer(line)?;
        buf.advance(consumed);
        Ok(RespValue::Integer(num))
    }

    /// Parse a bulk string: `$6\r\nfoobar\r\n` or `$-1\r\n` for null
    fn parse_bulk_string(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '$'
        let (line, consumed) = extract_line(buf)?;
        let length = parse_integer(line)?;
        buf.advance(consumed);

        // Handle null bulk string
        if length == -1 {
            return Ok(RespValue::Null);
        }

        if length < -1 {
            return Err(ParseError::InvalidBulkStringLength(length));
        }

        let length = length as usize;

        // Check if we have enough data
        if buf.len() < length + 2 {
            return Err(ParseError::Incomplete);
        }

        // Extract the bulk string data (zero-copy)
        let data = buf.split_to(length).freeze();

        // Verify and consume CRLF
        if buf.len() < 2 || &buf[0..2] != CRLF {
            return Err(ParseError::InvalidFormat(
                "Missing CRLF after bulk string".to_string(),
            ));
        }
        buf.advance(2);

        Ok(RespValue::BulkString(data))
    }

    /// Parse an array: `*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n` or `*-1\r\n` for null
    fn parse_array(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '*'
        let (line, consumed) = extract_line(buf)?;
        let length = parse_integer(line)?;
        buf.advance(consumed);

        // Handle null array
        if length == -1 {
            return Ok(RespValue::Null);
        }

        if length < -1 {
            return Err(ParseError::InvalidArrayLength(length));
        }

        let length = length as usize;
        let mut array = Vec::with_capacity(length);

        for _ in 0..length {
            let value = self.parse_value(buf)?;
            array.push(value);
        }

        Ok(RespValue::Array(array))
    }

    /// Parse null: `_\r\n`
    fn parse_null(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '_'
        let (_, consumed) = extract_line(buf)?;
        buf.advance(consumed);
        Ok(RespValue::Null)
    }

    /// Helper function to parse bulk data with length prefix
    /// Returns the data as Bytes after consuming length line and CRLF
    fn parse_bulk_with_length(&mut self, buf: &mut BytesMut) -> Result<Bytes, ParseError> {
        let (line, consumed) = extract_line(buf)?;
        let length = parse_integer(line)? as usize;
        buf.advance(consumed);

        if buf.len() < length + 2 {
            return Err(ParseError::Incomplete);
        }

        let data = buf.split_to(length).freeze();

        if buf.len() < 2 || &buf[0..2] != CRLF {
            return Err(ParseError::InvalidFormat(
                "Missing CRLF after bulk data".to_string(),
            ));
        }
        buf.advance(2);

        Ok(data)
    }

    /// Parse boolean: `#t\r\n` or `#f\r\n`
    fn parse_boolean(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '#'
        let (line, consumed) = extract_line(buf)?;

        let value = match line {
            b"t" => true,
            b"f" => false,
            _ => {
                return Err(ParseError::InvalidFormat(
                    "Boolean must be 't' or 'f'".to_string(),
                ));
            }
        };

        buf.advance(consumed);
        Ok(RespValue::Boolean(value))
    }

    /// Parse double: `,3.14\r\n` or `,inf\r\n`
    fn parse_double(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip ','
        let (line, consumed) = extract_line(buf)?;
        let value = parse_double(line)?;
        buf.advance(consumed);
        Ok(RespValue::Double(value))
    }

    /// Parse big number: `(3492890328409238509324850943850943825024385\r\n`
    fn parse_big_number(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '('
        let (line, consumed) = extract_line(buf)?;
        let value = Bytes::copy_from_slice(line);
        buf.advance(consumed);
        Ok(RespValue::BigNumber(value))
    }

    /// Parse bulk error: `!21\r\nSYNTAX invalid syntax\r\n`
    fn parse_bulk_error(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '!'
        let data = self.parse_bulk_with_length(buf)?;
        Ok(RespValue::BulkError(data))
    }

    /// Parse verbatim string: `=15\r\ntxt:Some string\r\n`
    fn parse_verbatim_string(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '='
        let data = self.parse_bulk_with_length(buf)?;

        // Split format and data (format is first 3 bytes + ':')
        if data.len() < 4 || data[3] != b':' {
            return Err(ParseError::InvalidFormat(
                "Verbatim string must have format prefix".to_string(),
            ));
        }

        let format = data.slice(0..3);
        let content = data.slice(4..);

        Ok(RespValue::VerbatimString {
            format,
            data: content,
        })
    }

    /// Parse map: `%2\r\n+first\r\n:1\r\n+second\r\n:2\r\n`
    fn parse_map(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '%'
        let (line, consumed) = extract_line(buf)?;
        let length = parse_integer(line)? as usize;
        buf.advance(consumed);

        let mut map = HashMap::with_capacity(length);

        for _ in 0..length {
            let key = self.parse_value(buf)?;
            let value = self.parse_value(buf)?;
            map.insert(key, value);
        }

        Ok(RespValue::Map(map))
    }

    /// Parse set: `~5\r\n+orange\r\n+apple\r\n...\r\n`
    fn parse_set(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '~'
        let (line, consumed) = extract_line(buf)?;
        let length = parse_integer(line)? as usize;
        buf.advance(consumed);

        let mut set = HashSet::with_capacity(length);

        for _ in 0..length {
            let value = self.parse_value(buf)?;
            set.insert(value);
        }

        Ok(RespValue::Set(set))
    }

    /// Parse push: `>4\r\n+pubsub\r\n+message\r\n...\r\n`
    fn parse_push(&mut self, buf: &mut BytesMut) -> Result<RespValue, ParseError> {
        buf.advance(1); // Skip '>'
        let (line, consumed) = extract_line(buf)?;
        let length = parse_integer(line)? as usize;
        buf.advance(consumed);

        let mut array = Vec::with_capacity(length);

        for _ in 0..length {
            let value = self.parse_value(buf)?;
            array.push(value);
        }

        Ok(RespValue::Push(array))
    }
}

impl Default for RespParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_string() {
        let mut parser = RespParser::new();
        let mut buf = BytesMut::from("+OK\r\n");
        let value = parser.parse(&mut buf).unwrap().unwrap();
        assert_eq!(value, RespValue::SimpleString(Bytes::from("OK")));
        assert!(buf.is_empty());
    }

    #[test]
    fn test_parse_error() {
        let mut parser = RespParser::new();
        let mut buf = BytesMut::from("-ERR unknown command\r\n");
        let value = parser.parse(&mut buf).unwrap().unwrap();
        assert_eq!(value, RespValue::Error(Bytes::from("ERR unknown command")));
    }

    #[test]
    fn test_parse_integer() {
        let mut parser = RespParser::new();
        let mut buf = BytesMut::from(":1000\r\n");
        let value = parser.parse(&mut buf).unwrap().unwrap();
        assert_eq!(value, RespValue::Integer(1000));
    }

    #[test]
    fn test_parse_bulk_string() {
        let mut parser = RespParser::new();
        let mut buf = BytesMut::from("$6\r\nfoobar\r\n");
        let value = parser.parse(&mut buf).unwrap().unwrap();
        assert_eq!(value, RespValue::BulkString(Bytes::from("foobar")));
    }

    #[test]
    fn test_parse_null_bulk_string() {
        let mut parser = RespParser::new();
        let mut buf = BytesMut::from("$-1\r\n");
        let value = parser.parse(&mut buf).unwrap().unwrap();
        assert_eq!(value, RespValue::Null);
    }

    #[test]
    fn test_parse_array() {
        let mut parser = RespParser::new();
        let mut buf = BytesMut::from("*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n");
        let value = parser.parse(&mut buf).unwrap().unwrap();

        match value {
            RespValue::Array(arr) => {
                assert_eq!(arr.len(), 2);
                assert_eq!(arr[0], RespValue::BulkString(Bytes::from("foo")));
                assert_eq!(arr[1], RespValue::BulkString(Bytes::from("bar")));
            }
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_parse_incomplete() {
        let mut parser = RespParser::new();
        let mut buf = BytesMut::from("+OK");
        let result = parser.parse(&mut buf).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_boolean() {
        let mut parser = RespParser::new();
        let mut buf = BytesMut::from("#t\r\n");
        let value = parser.parse(&mut buf).unwrap().unwrap();
        assert_eq!(value, RespValue::Boolean(true));
    }

    #[test]
    fn test_parse_double() {
        let mut parser = RespParser::new();
        let mut buf = BytesMut::from(",3.14\r\n");
        let value = parser.parse(&mut buf).unwrap().unwrap();
        assert_eq!(value, RespValue::Double(3.14));
    }
}
