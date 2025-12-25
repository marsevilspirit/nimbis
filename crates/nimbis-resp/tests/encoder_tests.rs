//! Integration tests for RESP encoder

use bytes::Bytes;
use resp::{RespEncoder, RespValue};

#[test]
fn test_encode_redis_ping() {
    let cmd = RespValue::Array(vec![RespValue::BulkString(Bytes::from("PING"))]);

    let encoded = cmd.encode().unwrap();
    assert_eq!(&encoded[..], b"*1\r\n$4\r\nPING\r\n");
}

#[test]
fn test_encode_redis_set() {
    let cmd = RespValue::Array(vec![
        RespValue::BulkString(Bytes::from("SET")),
        RespValue::BulkString(Bytes::from("key")),
        RespValue::BulkString(Bytes::from("value")),
    ]);

    let encoded = cmd.encode().unwrap();
    assert_eq!(
        &encoded[..],
        b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n"
    );
}

#[test]
fn test_encode_redis_get() {
    let cmd = RespValue::Array(vec![
        RespValue::BulkString(Bytes::from("GET")),
        RespValue::BulkString(Bytes::from("key")),
    ]);

    let encoded = cmd.encode().unwrap();
    assert_eq!(&encoded[..], b"*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n");
}

#[test]
fn test_roundtrip_simple_types() {
    let test_cases = vec![
        RespValue::SimpleString(Bytes::from("OK")),
        RespValue::Error(Bytes::from("ERR test error")),
        RespValue::Integer(42),
        RespValue::Integer(-100),
        RespValue::BulkString(Bytes::from("hello world")),
        RespValue::Null,
    ];

    for original in test_cases {
        let encoded = original.encode().unwrap();
        let decoded = resp::parse(&encoded).unwrap();
        assert_eq!(original, decoded, "Roundtrip failed for {:?}", original);
    }
}

#[test]
fn test_roundtrip_arrays() {
    let original = RespValue::Array(vec![
        RespValue::SimpleString(Bytes::from("OK")),
        RespValue::Integer(123),
        RespValue::BulkString(Bytes::from("test")),
    ]);

    let encoded = original.encode().unwrap();
    let decoded = resp::parse(&encoded).unwrap();
    assert_eq!(original, decoded);
}

#[test]
fn test_roundtrip_nested_arrays() {
    let original = RespValue::Array(vec![
        RespValue::Array(vec![RespValue::Integer(1), RespValue::Integer(2)]),
        RespValue::Array(vec![RespValue::Integer(3), RespValue::Integer(4)]),
    ]);

    let encoded = original.encode().unwrap();
    let decoded = resp::parse(&encoded).unwrap();
    assert_eq!(original, decoded);
}

#[test]
fn test_roundtrip_resp3_types() {
    let test_cases = vec![
        RespValue::Boolean(true),
        RespValue::Boolean(false),
        RespValue::Double(3.14159),
        RespValue::Double(f64::INFINITY),
        RespValue::Double(f64::NEG_INFINITY),
        RespValue::BigNumber(Bytes::from("123456789012345678901234567890")),
    ];

    for original in test_cases {
        let encoded = original.encode().unwrap();
        let decoded = resp::parse(&encoded).unwrap();
        assert_eq!(original, decoded, "Roundtrip failed for {:?}", original);
    }
}

#[test]
fn test_encode_empty_array() {
    let value = RespValue::Array(vec![]);
    let encoded = value.encode().unwrap();
    assert_eq!(&encoded[..], b"*0\r\n");
}

#[test]
fn test_encode_empty_bulk_string() {
    let value = RespValue::BulkString(Bytes::new());
    let encoded = value.encode().unwrap();
    assert_eq!(&encoded[..], b"$0\r\n\r\n");
}

#[test]
fn test_encode_large_bulk_string() {
    let data = "x".repeat(1024);
    let value = RespValue::BulkString(Bytes::from(data.clone()));
    let encoded = value.encode().unwrap();

    let decoded = resp::parse(&encoded).unwrap();
    assert_eq!(decoded.as_bytes().unwrap(), &Bytes::from(data));
}

#[test]
fn test_encode_binary_data() {
    let data: Vec<u8> = (0..=255).collect();
    let value = RespValue::BulkString(Bytes::from(data.clone()));
    let encoded = value.encode().unwrap();

    let decoded = resp::parse(&encoded).unwrap();
    assert_eq!(decoded.as_bytes().unwrap(), &Bytes::from(data));
}
