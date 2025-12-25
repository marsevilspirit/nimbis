//! Basic usage examples for the RESP library

use bytes::Bytes;
use resp::{RespEncoder, RespValue};

fn main() {
    println!("=== RESP Library Basic Usage Examples ===\n");

    // Example 1: Parsing a simple string
    example_parse_simple_string();

    // Example 2: Parsing a Redis command
    example_parse_redis_command();

    // Example 3: Creating and encoding RESP values
    example_create_and_encode();

    // Example 4: Round-trip (encode -> parse)
    example_roundtrip();

    // Example 5: Working with different types
    example_different_types();
}

fn example_parse_simple_string() {
    println!("--- Example 1: Parse Simple String ---");

    let value = resp::parse(b"+OK\r\n").unwrap();
    println!("Parsed: {:?}", value);
    println!("As string: {:?}", value.as_str());
    println!();
}

fn example_parse_redis_command() {
    println!("--- Example 2: Parse Redis Command ---");

    let value = resp::parse(b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n").unwrap();
    println!("Parsed: {:?}", value);

    if let Some(array) = value.as_array() {
        println!("Command parts:");
        for (i, part) in array.iter().enumerate() {
            println!("  [{}]: {:?}", i, part.as_str());
        }
    }
    println!();
}

fn example_create_and_encode() {
    println!("--- Example 3: Create and Encode ---");

    // Create a Redis SET command
    let cmd = RespValue::Array(vec![
        RespValue::BulkString(Bytes::from("SET")),
        RespValue::BulkString(Bytes::from("mykey")),
        RespValue::BulkString(Bytes::from("myvalue")),
    ]);

    let encoded = cmd.encode().unwrap();
    println!("Encoded command:");
    println!("{:?}", String::from_utf8_lossy(&encoded));
    println!("Bytes: {:?}", encoded);
    println!();
}

fn example_roundtrip() {
    println!("--- Example 4: Round-trip ---");

    let original = RespValue::Array(vec![
        RespValue::SimpleString(Bytes::from("HELLO")),
        RespValue::Integer(42),
        RespValue::BulkString(Bytes::from("world")),
    ]);

    println!("Original: {:?}", original);

    // Encode
    let encoded = original.encode().unwrap();
    println!(
        "Encoded ({} bytes): {:?}",
        encoded.len(),
        String::from_utf8_lossy(&encoded)
    );

    // Parse back
    let decoded = resp::parse(&encoded).unwrap();
    println!("Decoded: {:?}", decoded);

    println!("Match: {}", original == decoded);
    println!();
}

fn example_different_types() {
    println!("--- Example 5: Different Types ---");

    let types = vec![
        ("Simple String", RespValue::SimpleString(Bytes::from("OK"))),
        (
            "Error",
            RespValue::Error(Bytes::from("ERR something went wrong")),
        ),
        ("Integer", RespValue::Integer(1234)),
        (
            "Bulk String",
            RespValue::BulkString(Bytes::from("Hello, RESP!")),
        ),
        ("Null", RespValue::Null),
        ("Boolean (RESP3)", RespValue::Boolean(true)),
        ("Double (RESP3)", RespValue::Double(3.14159)),
    ];

    for (name, value) in types {
        let encoded = value.encode().unwrap();
        println!("{}: {:?}", name, String::from_utf8_lossy(&encoded).trim());
    }
    println!();
}
