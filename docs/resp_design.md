# Nimbis-RESP Implementation Design and Usage Guide

`nimbis-resp` is a high-performance, zero-copy Redis Serialization Protocol (RESP) parser and encoder, supporting both RESP2 and RESP3 standards. This library is designed for high-performance servers and clients, strictly leveraging Rust's memory safety and type system.

## 1. Implementation Principles

### 1.1 Core Architecture

`nimbis-resp` follows a single-responsibility principle, composed of several core modules:

- **Types (`types.rs`)**: Defines a unified `RespValue` enum capable of representing all data types for both RESP2 and RESP3.
- **Parser (`parser.rs`)**: Responsible for parsing byte streams into `RespValue`, utilizing zero-copy techniques.
- **Encoder (`encoder.rs`)**: Responsible for serializing `RespValue` into byte streams and writing them to a buffer.
- **Error (`error.rs`)**: Provides detailed error types, distinguishing between parsing and encoding errors.

### 1.2 Inline Command Support

`nimbis-resp` supports **inline commands**, which are simple text-based commands used for debugging and telnet-style connections. This is an alternative to the binary RESP protocol format.

**Format:** `COMMAND arg1 arg2 ...\r\n`

**Features:**
- Commands are parsed by splitting on whitespace
- Maximum command length: 64KB (preventing DoS attacks)
- UTF-8 validation for all arguments
- Empty lines and whitespace-only lines are ignored
- The first character must be a printable ASCII character (0x21-0x7E) or space

**Limitations:**
- Does not support quoted strings with spaces (e.g., `SET key "value with spaces"` will be parsed as `["SET", "key", "\"value", "with", "spaces\""]`)

### 1.3 Zero-copy Design

To achieve extreme performance, `nimbis-resp` extensively uses the `bytes` crate.

- **Parsing Process**: When parsing Bulk Strings or other types carrying data, the parser does not copy the data but returns a `Bytes` object. `Bytes` is a reference-counted handle to the underlying memory, making the passing of strings and binary data almost cost-free.
- **Memory View**: After reading data from a TCP stream into a `BytesMut` buffer, the parsed `RespValue` merely holds a slice of this buffer. Data is only copied when the user explicitly takes ownership (e.g., converting to a String).

### 1.4 Type System and Enums

The `RespValue` enum is the core data structure of the library, unifying RESP2 and RESP3 handling:

```rust
pub enum RespValue {
    // RESP2
    SimpleString(Bytes),
    Error(Bytes),
    Integer(i64),
    BulkString(Bytes),
    Array(Vec<RespValue>),
    Null, // Correspond to RESP2 null bulk string and RESP3 null
    
    // RESP3 Extensions
    Boolean(bool),
    Double(f64),
    Map(HashMap<RespValue, RespValue>),
    Set(HashSet<RespValue>),
    // ... other types
}
```

This design makes handling polymorphic responses simple and safe, allowing elegant handling of various Redis return values using Rust's pattern matching.

### 1.5 Parsing and Encoding Mechanisms

- **Parser (`RespParser`)**: Uses a stateful, resumable parsing strategy.
    1. Maintains a stack of frames to track nested structures (Arrays, Maps, etc.).
    2. Uses `peek_line` to check for complete data before consuming.
    3. Returns `RespParseResult` to indicate `Complete`, `Incomplete`, or `Error` states.
    
- **Encoder**: Implements the `RespEncoder` trait.
    - Provides an `encode_to` method to write data into a mutable `BytesMut` buffer.
    - Provides an `encode` convenience method to return `Bytes` directly.
    - Optimized with pre-calculation for various types to minimize memory allocations.

---

## 2. Usage Guide

### 2.1 Installation

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
nimbis-resp = { path = "crates/nimbis-resp" } # Or specify version/git url
bytes = "1.5"
```

### 2.2 Streaming Parsing (Recommended)

For TCP servers, use `RespParser` to handle streaming data, allowing for partial reads and resumable parsing:

```rust
use bytes::BytesMut;
use resp::{RespParser, RespParseResult, RespValue};

fn main() {
    let mut parser = RespParser::new();
    let mut buf = BytesMut::from(&b"*2\r\n$3\r\nSET"[..]); // Incomplete data

    // First attempt: Incomplete
    match parser.parse(&mut buf) {
        RespParseResult::Incomplete => println!("Need more data..."),
        _ => panic!("Should be incomplete"),
    }

    // Append more data
    buf.extend_from_slice(b"\r\n$3\r\nkey\r\n");
    
    // Second attempt: Complete
    loop {
        match parser.parse(&mut buf) {
            RespParseResult::Complete(val) => {
                println!("Parsed: {:?}", val);
                // Handle value...
            },
            RespParseResult::Incomplete => break,
            RespParseResult::Error(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }
}
```

### 2.3 One-off Parsing

For simple cases where you have a full buffer, you can use the `resp::parse` helper:

```rust
use bytes::BytesMut;
use resp::RespValue;

fn main() {
    let mut buf = BytesMut::from(&b"+OK\r\n"[..]);
    let value = resp::parse(&mut buf).unwrap();
    
    assert_eq!(value.as_str(), Some("OK"));
}
```

### 2.4 Creation and Encoding

You can construct `RespValue` directly using variants or use convenience constructors:

```rust
use resp::RespValue;
use bytes::Bytes;

fn main() {
    // Method 1: Using convenience constructors
    let cmd = RespValue::array(vec![
        RespValue::bulk_string("SET"),
        RespValue::bulk_string("key"),
        RespValue::bulk_string("value"),
    ]);

    // Method 2: Using From trait
    let cmd2: RespValue = vec![
        "GET".into(),
        "key".into(),
    ].into();

    // Encode to bytes
    let encoded_bytes = cmd.encode().unwrap();
    
    // Output: *3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n
    println!("{:?}", String::from_utf8_lossy(&encoded_bytes));
}
```

### 2.5 Handling Complex Types (Arrays & Maps)

```rust
use resp::RespValue;

fn handle_response(val: RespValue) {
    match val {
        RespValue::Array(items) => {
            println!("Received array with {} items:", items.len());
            for item in items {
                println!(" - {:?}", item);
            }
        },
        RespValue::Map(map) => {
            println!("Received map:");
            for (k, v) in map {
                println!(" Key: {:?}, Value: {:?}", k, v);
            }
        },
        RespValue::Error(err) => {
            println!("Redis Error: {:?}", String::from_utf8_lossy(&err));
        },
        _ => println!("Other: {:?}", val),
    }
}
```

### 2.6 Helper Methods

`RespValue` provides various helper methods to simplify data extraction from the enum:

- `as_str()`: Try converting to `&str`
- `as_integer()`: Try converting to `i64`
- `as_array()`: Try getting an array reference
- `as_bool()`: Try converting to `bool`
- `to_string_lossy()`: Convert to `String` (handling non-UTF-8 data)

```rust
let val = RespValue::integer(42);
if let Some(num) = val.as_integer() {
    println!("Number is {}", num);
}
```

## 3. Performance Tips

1. **Reuse Buffers**: In high-concurrency scenarios, reuse `BytesMut` buffers for reading network data to avoid frequent memory allocations.
2. **Use Bytes**: When constructing `RespValue`, prefer `Bytes::from_static` or existing `Bytes` objects to leverage zero-copy features.
3. **Pre-allocation**: Use `Vec::with_capacity` to pre-allocate memory when building large arrays or maps.
