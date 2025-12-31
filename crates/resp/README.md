# RESP - Redis Serialization Protocol Parser

A high-performance, zero-copy, streaming RESP protocol parser written in Rust.

## Features

- ⚡ **Zero-copy parsing** - Efficient memory management using `Bytes`
- 🌊 **Streaming Support** - Native support for fragmented TCP streams
- 🔧 **RESP2 & RESP3 support** - Complete protocol support
- 🔒 **Type-safe** - Leverages Rust's type system
- 🚀 **High performance** - Optimized for throughput and minimal allocations

## Usage Examples

### Streaming Parsing (Recommended)

The `RespParser` is designed to handle streaming data where frames might be fragmented across multiple buffers.

```rust
use bytes::BytesMut;
use resp::{RespParser, RespParseResult, RespValue};

let mut parser = RespParser::new();
let mut buf = BytesMut::new();

// Simulate receiving data in chunks
buf.extend_from_slice(b"*2\r\n$3\r\nSET\r\n");

// Attempt to parse
loop {
    match parser.parse(&mut buf) {
        RespParseResult::Complete(value) => {
            println!("Parsed: {:?}", value);
            // Process value...
        },
        RespParseResult::Incomplete => {
            // Wait for more data
            break;
        },
        RespParseResult::Error(e) => {
            eprintln!("Error: {:?}", e);
            break;
        }
    }
}
```

### Simple One-off Parsing

For simple cases where you have the full data:

```rust
use bytes::BytesMut;
use resp;

let mut buf = BytesMut::from(&b"+OK\r\n"[..]);
let value = resp::parse(&mut buf).unwrap();
assert_eq!(value.as_str(), Some("OK"));
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
# Run streaming parse example
cargo run -p resp --example parse_stream
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

## Architecture

```
resp/
├── src/
│   ├── lib.rs          # Library entry point
│   ├── types.rs        # RESP value type definitions
│   ├── parser.rs       # Parser implementation
│   ├── error.rs        # Error types
│   └── utils.rs        # Utility functions
├── tests/              # Integration tests
├── benches/            # Performance benchmarks
└── examples/           # Example code
```