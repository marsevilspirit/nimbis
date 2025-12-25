//! Integration tests for RESP parser

use bytes::BytesMut;
use resp::{RespParser, RespValue};

#[test]
fn test_parse_redis_ping() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from("*1\r\n$4\r\nPING\r\n");

    let value = parser.parse(&mut buf).unwrap().unwrap();

    match value {
        RespValue::Array(arr) => {
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0].as_str(), Some("PING"));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_parse_redis_set() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from("*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n");

    let value = parser.parse(&mut buf).unwrap().unwrap();

    match value {
        RespValue::Array(arr) => {
            assert_eq!(arr.len(), 3);
            assert_eq!(arr[0].as_str(), Some("SET"));
            assert_eq!(arr[1].as_str(), Some("key"));
            assert_eq!(arr[2].as_str(), Some("value"));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_parse_redis_get_response() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from("$5\r\nvalue\r\n");

    let value = parser.parse(&mut buf).unwrap().unwrap();
    assert_eq!(value.as_str(), Some("value"));
}

#[test]
fn test_parse_redis_nil_response() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from("$-1\r\n");

    let value = parser.parse(&mut buf).unwrap().unwrap();
    assert!(value.is_null());
}

#[test]
fn test_parse_redis_ok_response() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from("+OK\r\n");

    let value = parser.parse(&mut buf).unwrap().unwrap();
    assert_eq!(value.as_str(), Some("OK"));
}

#[test]
fn test_parse_redis_error_response() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from("-ERR unknown command 'foobar'\r\n");

    let value = parser.parse(&mut buf).unwrap().unwrap();
    assert!(value.is_error());
    assert_eq!(
        value,
        RespValue::Error("ERR unknown command 'foobar'".into())
    );
}

// NOTE: True incremental parsing (where incomplete data doesn't consume bytes)
// requires parser refactoring to use lookahead without consuming.
// Current implementation consumes type markers before validating completeness.

#[test]
fn test_parse_multiple_values() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from("+OK\r\n+PONG\r\n:42\r\n");

    // Parse first value
    let value1 = parser.parse(&mut buf).unwrap().unwrap();
    assert_eq!(value1.as_str(), Some("OK"));

    // Parse second value
    let value2 = parser.parse(&mut buf).unwrap().unwrap();
    assert_eq!(value2.as_str(), Some("PONG"));

    // Parse third value
    let value3 = parser.parse(&mut buf).unwrap().unwrap();
    assert_eq!(value3.as_integer(), Some(42));

    // Buffer should be empty
    assert!(buf.is_empty());
}

#[test]
fn test_parse_nested_arrays() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from("*2\r\n*2\r\n:1\r\n:2\r\n*2\r\n:3\r\n:4\r\n");

    let value = parser.parse(&mut buf).unwrap().unwrap();

    match value {
        RespValue::Array(outer) => {
            assert_eq!(outer.len(), 2);

            match &outer[0] {
                RespValue::Array(inner) => {
                    assert_eq!(inner.len(), 2);
                    assert_eq!(inner[0].as_integer(), Some(1));
                    assert_eq!(inner[1].as_integer(), Some(2));
                }
                _ => panic!("Expected inner array"),
            }

            match &outer[1] {
                RespValue::Array(inner) => {
                    assert_eq!(inner.len(), 2);
                    assert_eq!(inner[0].as_integer(), Some(3));
                    assert_eq!(inner[1].as_integer(), Some(4));
                }
                _ => panic!("Expected inner array"),
            }
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_parse_empty_array() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from("*0\r\n");

    let value = parser.parse(&mut buf).unwrap().unwrap();
    match value {
        RespValue::Array(arr) => assert_eq!(arr.len(), 0),
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_parse_empty_bulk_string() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from("$0\r\n\r\n");

    let value = parser.parse(&mut buf).unwrap().unwrap();
    assert_eq!(value.as_str(), Some(""));
}

#[test]
fn test_resp3_types() {
    let mut parser = RespParser::new();

    // Boolean true
    let mut buf = BytesMut::from("#t\r\n");
    let value = parser.parse(&mut buf).unwrap().unwrap();
    assert_eq!(value.as_bool(), Some(true));

    // Boolean false
    let mut buf = BytesMut::from("#f\r\n");
    let value = parser.parse(&mut buf).unwrap().unwrap();
    assert_eq!(value.as_bool(), Some(false));

    // Double
    let mut buf = BytesMut::from(",3.14159\r\n");
    let value = parser.parse(&mut buf).unwrap().unwrap();
    assert_eq!(value.as_double(), Some(3.14159));

    // Null
    let mut buf = BytesMut::from("_\r\n");
    let value = parser.parse(&mut buf).unwrap().unwrap();
    assert!(value.is_null());
}
