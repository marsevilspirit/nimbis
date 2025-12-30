//! RESP data types and value representation.

use std::collections::HashMap;
use std::collections::HashSet;

use bytes::Bytes;

/// Represents a RESP protocol value.
///
/// Supports both RESP2 and RESP3 types.
#[derive(Debug, Clone, PartialEq)]
pub enum RespValue {
    // RESP2 types
    /// Simple string: `+OK\r\n`
    SimpleString(Bytes),

    /// Error: `-ERR message\r\n`
    Error(Bytes),

    /// Integer: `:1000\r\n`
    Integer(i64),

    /// Bulk string: `$6\r\nfoobar\r\n`
    BulkString(Bytes),

    /// Array: `*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n`
    Array(Vec<RespValue>),

    /// Null: `$-1\r\n` (RESP2) or `_\r\n` (RESP3)
    Null,

    // RESP3 types
    /// Boolean: `#t\r\n` or `#f\r\n`
    Boolean(bool),

    /// Double: `,3.14\r\n`
    Double(f64),

    /// Big number: `(3492890328409238509324850943850943825024385\r\n`
    BigNumber(Bytes),

    /// Bulk error: `!21\r\nSYNTAX invalid syntax\r\n`
    BulkError(Bytes),

    /// Verbatim string: `=15\r\ntxt:Some string\r\n`
    VerbatimString { format: Bytes, data: Bytes },

    /// Map: `%2\r\n+first\r\n:1\r\n+second\r\n:2\r\n`
    Map(HashMap<RespValue, RespValue>),

    /// Set: `~5\r\n+orange\r\n+apple\r\n...\r\n`
    Set(HashSet<RespValue>),

    /// Push: `>4\r\n+pubsub\r\n+message\r\n...\r\n`
    Push(Vec<RespValue>),
}

impl RespValue {
    /// Check if the value is an error
    pub fn is_error(&self) -> bool {
        matches!(self, RespValue::Error(_) | RespValue::BulkError(_))
    }

    /// Check if the value is null
    pub fn is_null(&self) -> bool {
        matches!(self, RespValue::Null)
    }

    /// Try to convert to a string slice
    pub fn as_str(&self) -> Option<&str> {
        match self {
            RespValue::SimpleString(s) | RespValue::BulkString(s) => std::str::from_utf8(s).ok(),
            _ => None,
        }
    }

    /// Try to convert to bytes
    pub fn as_bytes(&self) -> Option<&Bytes> {
        match self {
            RespValue::SimpleString(b) | RespValue::BulkString(b) => Some(b),
            _ => None,
        }
    }

    /// Try to convert to integer
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            RespValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Try to convert to array
    pub fn as_array(&self) -> Option<&Vec<RespValue>> {
        match self {
            RespValue::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Try to convert to boolean
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            RespValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Try to convert to double
    pub fn as_double(&self) -> Option<f64> {
        match self {
            RespValue::Double(d) => Some(*d),
            _ => None,
        }
    }

    /// Try to convert to map
    pub fn as_map(&self) -> Option<&HashMap<RespValue, RespValue>> {
        match self {
            RespValue::Map(m) => Some(m),
            _ => None,
        }
    }

    /// Convert to String with lossy UTF-8 conversion
    pub fn to_string_lossy(&self) -> Option<String> {
        match self {
            RespValue::SimpleString(s) | RespValue::BulkString(s) => {
                Some(String::from_utf8_lossy(s).into_owned())
            }
            _ => None,
        }
    }

    /// Try to consume and convert to Vec<RespValue>
    pub fn into_vec(self) -> Option<Vec<RespValue>> {
        match self {
            RespValue::Array(a) | RespValue::Push(a) => Some(a),
            _ => None,
        }
    }

    // Convenience constructors

    /// Create a simple string value
    pub fn simple_string(s: impl Into<Bytes>) -> Self {
        RespValue::SimpleString(s.into())
    }

    /// Create a bulk string value
    pub fn bulk_string(s: impl Into<Bytes>) -> Self {
        RespValue::BulkString(s.into())
    }

    /// Create an error value
    pub fn error(e: impl Into<Bytes>) -> Self {
        RespValue::Error(e.into())
    }

    /// Create an integer value
    pub fn integer(i: i64) -> Self {
        RespValue::Integer(i)
    }

    /// Create an array value from an iterator
    pub fn array(items: impl IntoIterator<Item = RespValue>) -> Self {
        RespValue::Array(items.into_iter().collect())
    }

    /// Create a null value
    pub fn null() -> Self {
        RespValue::Null
    }
}

// Implement Hash for RespValue (needed for Set and Map keys)
impl std::hash::Hash for RespValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            RespValue::SimpleString(s) | RespValue::BulkString(s) => {
                0u8.hash(state);
                s.hash(state);
            }
            RespValue::Error(e) => {
                1u8.hash(state);
                e.hash(state);
            }
            RespValue::Integer(i) => {
                2u8.hash(state);
                i.hash(state);
            }
            RespValue::Null => 3u8.hash(state),
            RespValue::Boolean(b) => {
                4u8.hash(state);
                b.hash(state);
            }
            RespValue::Double(d) => {
                5u8.hash(state);
                d.to_bits().hash(state);
            }
            RespValue::BigNumber(n) => {
                6u8.hash(state);
                n.hash(state);
            }
            RespValue::BulkError(e) => {
                7u8.hash(state);
                e.hash(state);
            }
            RespValue::VerbatimString { format, data } => {
                8u8.hash(state);
                format.hash(state);
                data.hash(state);
            }
            RespValue::Array(_) | RespValue::Map(_) | RespValue::Set(_) | RespValue::Push(_) => {
                // Collections can't be hashed easily, use type discriminant
                std::mem::discriminant(self).hash(state);
            }
        }
    }
}

impl Eq for RespValue {}

// Convenient From implementations
impl From<&str> for RespValue {
    fn from(s: &str) -> Self {
        RespValue::BulkString(Bytes::from(s.to_string()))
    }
}

impl From<String> for RespValue {
    fn from(s: String) -> Self {
        RespValue::BulkString(Bytes::from(s))
    }
}

impl From<&[u8]> for RespValue {
    fn from(b: &[u8]) -> Self {
        RespValue::BulkString(Bytes::copy_from_slice(b))
    }
}

impl From<Vec<u8>> for RespValue {
    fn from(v: Vec<u8>) -> Self {
        RespValue::BulkString(Bytes::from(v))
    }
}

impl From<i64> for RespValue {
    fn from(i: i64) -> Self {
        RespValue::Integer(i)
    }
}

impl From<i32> for RespValue {
    fn from(i: i32) -> Self {
        RespValue::Integer(i as i64)
    }
}

impl From<bool> for RespValue {
    fn from(b: bool) -> Self {
        RespValue::Boolean(b)
    }
}

impl From<f64> for RespValue {
    fn from(d: f64) -> Self {
        RespValue::Double(d)
    }
}

impl From<Bytes> for RespValue {
    fn from(b: Bytes) -> Self {
        RespValue::BulkString(b)
    }
}

impl<T: Into<RespValue>> From<Vec<T>> for RespValue {
    fn from(v: Vec<T>) -> Self {
        RespValue::Array(v.into_iter().map(|x| x.into()).collect())
    }
}

impl<T: Into<RespValue>> From<Option<T>> for RespValue {
    fn from(o: Option<T>) -> Self {
        match o {
            Some(v) => v.into(),
            None => RespValue::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_error() {
        let err = RespValue::Error(Bytes::from("ERR"));
        assert!(err.is_error());

        let ok = RespValue::SimpleString(Bytes::from("OK"));
        assert!(!ok.is_error());
    }

    #[test]
    fn test_as_str() {
        let val = RespValue::SimpleString(Bytes::from("hello"));
        assert_eq!(val.as_str(), Some("hello"));

        let num = RespValue::Integer(42);
        assert_eq!(num.as_str(), None);
    }

    #[test]
    fn test_from_conversions() {
        let s: RespValue = "test".into();
        assert_eq!(s.as_str(), Some("test"));

        let i: RespValue = 42i64.into();
        assert_eq!(i.as_integer(), Some(42));

        let b: RespValue = true.into();
        assert_eq!(b.as_bool(), Some(true));
    }

    #[test]
    fn test_convenience_constructors() {
        let s = RespValue::simple_string("OK");
        assert_eq!(s.as_str(), Some("OK"));

        let b = RespValue::bulk_string("hello");
        assert_eq!(b.as_str(), Some("hello"));

        let e = RespValue::error("ERR");
        assert!(e.is_error());

        let i = RespValue::integer(42);
        assert_eq!(i.as_integer(), Some(42));

        let arr = RespValue::array(vec![RespValue::integer(1), RespValue::integer(2)]);
        assert_eq!(arr.as_array().map(|a| a.len()), Some(2));

        let n = RespValue::null();
        assert!(n.is_null());
    }

    #[test]
    fn test_to_string_lossy() {
        let val = RespValue::bulk_string("hello");
        assert_eq!(val.to_string_lossy(), Some("hello".to_string()));

        let num = RespValue::integer(42);
        assert_eq!(num.to_string_lossy(), None);
    }

    #[test]
    fn test_into_vec() {
        let arr = RespValue::array(vec![RespValue::integer(1), RespValue::integer(2)]);
        let vec = arr.into_vec().unwrap();
        assert_eq!(vec.len(), 2);
    }
}
