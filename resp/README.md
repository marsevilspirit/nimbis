# RESP - Redis Serialization Protocol Library

A high-performance, zero-copy RESP protocol parser and encoder written in Rust.

## Features

- ⚡ **Zero-copy parsing** - Efficient memory management using `Bytes`
- 🔧 **RESP2 & RESP3 support** - Complete protocol support
- 🔒 **Type-safe** - Leverages Rust's type system
- 🚀 **High performance** - Optimized for throughput and minimal allocations
- 📦 **Incremental parsing** - Efficient streaming data handling
- ✨ **Elegant API** - Ergonomic interface design

## Usage Examples

### Parsing RESP Values

```rust
use resp::{RespParser, RespValue};
use bytes::BytesMut;

let mut parser = RespParser::new();
let mut buf = BytesMut::from("+OK\r\n");

let value = parser.parse(&mut buf).unwrap().unwrap();
assert_eq!(value, RespValue::SimpleString("OK".into()));
```

### Creating and Encoding RESP Values

```rust
use resp::{RespValue, RespEncoder};
use bytes::Bytes;

// Create a Redis SET command
let cmd = RespValue::Array(vec![
    RespValue::BulkString(Bytes::from("SET")),
    RespValue::BulkString(Bytes::from("key")),
    RespValue::BulkString(Bytes::from("value")),
]);

// Encode to bytes
let encoded = cmd.encode().unwrap();
// Output: b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n"
```

### Incremental Parsing

```rust
use resp::RespParser;
use bytes::BytesMut;

let mut parser = RespParser::new();
let mut buf = BytesMut::new();

// Receive first chunk
buf.extend_from_slice(b"*3\r\n$3\r\nSET");
assert!(parser.parse(&mut buf).unwrap().is_none()); // Incomplete

// Receive more data
buf.extend_from_slice(b"\r\n$3\r\nkey\r\n$5\r\nvalue\r\n");
let value = parser.parse(&mut buf).unwrap().unwrap(); // Complete!
```

### Type Conversions

```rust
use resp::RespValue;

let value = RespValue::BulkString("hello".into());

// Safe type conversion
if let Some(s) = value.as_str() {
    println!("String value: {}", s);
}

// From implementations
let from_str: RespValue = "test".into();
let from_int: RespValue = 42i64.into();
let from_bool: RespValue = true.into();
```

## Supported Types

### RESP2 Types
- ✅ Simple String (`+OK\r\n`)
- ✅ Error (`-ERR message\r\n`)
- ✅ Integer (`:1000\r\n`)
- ✅ Bulk String (`$6\r\nfoobar\r\n`)
- ✅ Array (`*2\r\n...`)
- ✅ Null (`$-1\r\n`)

### RESP3 Types
- ✅ Boolean (`#t\r\n` / `#f\r\n`)
- ✅ Double (`,3.14\r\n`)
- ✅ Big Number (`(12345...\r\n`)
- ✅ Bulk Error (`!21\r\nERROR...\r\n`)
- ✅ Verbatim String (`=15\r\ntxt:...\r\n`)
- ✅ Map (`%2\r\n...`)
- ✅ Set (`~5\r\n...`)
- ✅ Push (`>4\r\n...`)

## Examples

See the `examples/` directory for more usage patterns:

```bash
# Basic usage example
cargo run --example basic_usage
```

## Running Tests

```bash
# Run all tests
just test
```

## Performance Benchmarks

```bash
just bench
```

Benchmarks include:
- Parsing performance for different RESP types
- Encoding performance
- Round-trip (encode + parse) performance
- Performance with large arrays and complex nested structures

## Development

```bash
# Build the library
just build

# Run all checks (format, clippy, test)
just all

# Check code and formatting
just check

# Format code
just fmt
```

## API Documentation

Generate and view API documentation:

```bash
cargo doc --no-deps --open
```

## Performance Optimizations

This library employs several optimization techniques:

1. **Zero-copy** - Uses `Bytes::slice()` to avoid unnecessary memory copies
2. **Early return** - Quick return when incomplete data is encountered
3. **Capacity pre-allocation** - Pre-allocates memory for collections of known size
4. **Minimal allocations** - Reuses buffers and avoids temporary allocations

## Architecture

```
resp/
├── src/
│   ├── lib.rs          # Library entry point
│   ├── types.rs        # RESP value type definitions
│   ├── parser.rs       # Parser implementation
│   ├── encoder.rs      # Encoder implementation
│   ├── error.rs        # Error types
│   └── utils.rs        # Utility functions
├── tests/              # Integration tests
├── benches/            # Performance benchmarks
└── examples/           # Example code
```