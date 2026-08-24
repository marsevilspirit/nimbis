# Storage Error Handling

## Overview

The storage crate uses a structured error handling system based on `thiserror`. Errors are categorized into two main enums: `DecoderError` for low-level decoding issues and `StorageError` for high-level storage operations.

## Error Codes

All errors include a unique error code for identification and logging:

| Code | Type | Description |
|------|------|-------------|
| E0001 | DecoderError | Empty key, cannot decode |
| E0002 | DecoderError | Invalid type code |
| E0003 | DecoderError | Invalid data length |
| E1000 | StorageError | Database operation failed |
| E1002 | StorageError | Failed to decode data |
| E1003 | StorageError | I/O operation failed |
| E1004 | StorageError | Data inconsistency detected |
| E1005 | StorageError | Invalid object-store configuration |
| E1006 | StorageError | Encoded key exceeds the storage limit |
| E1007 | StorageError | Expiration timestamp exceeds the supported range |

### Detailed Error Codes

When errors wrap other errors, `detailed_code()` returns a combined code:
- `DecodeError` wrapping `DecoderError::Empty` → `"E1002:E0001"`

## Error Types

### DecoderError

Low-level decoding errors from data parsing:

```rust
#[derive(Debug, Error)]
pub enum DecoderError {
    #[error("Empty key, cannot decode")]
    Empty,

    #[error("Invalid type code")]
    InvalidType,

    #[error("Invalid data length")]
    InvalidLength,
}
```

### StorageError

High-level storage operation errors:

```rust
#[derive(Debug, Error)]
pub enum StorageError {
    /// Database operation failed
    DatabaseError {
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Encoding/Decoding error
    DecodeError {
        source: DecoderError,
    },

    /// I/O operation failed
    IoError {
        source: std::io::Error,
    },

    /// Data inconsistency detected
    DataInconsistency { message: String },

    /// Object-store URL or options are invalid
    ObjectStoreConfig { message: String },

    /// Complete encoded key is too long for SlateDB
    InvalidKeyLength { length: usize, max: usize },

    /// Absolute expiration exceeds Nimbis's supported logical deadline
    InvalidExpiration { timestamp: u64, max: u64 },
}
```

## Error Conversions

The storage crate implements `From` traits for seamless error conversion:

| From | To |
|------|-----|
| `DecoderError` | `StorageError::DecodeError` |
| `std::io::Error` | `StorageError::IoError` |
| `Box<dyn Error + Send + Sync>` | `StorageError::DatabaseError` (or `IoError` if io::Error) |
| `slatedb::Error` | `StorageError::DatabaseError` |
| `slatedb::object_store::Error` | `StorageError::DatabaseError` |
| `std::str::Utf8Error` | `StorageError::DecodeError` (InvalidLength) |
| `std::array::TryFromSliceError` | `StorageError::DecodeError` (InvalidLength) |
| `std::num::ParseIntError` | `StorageError::DecodeError` (InvalidType) |

## Usage Examples

### Basic Error Handling

```rust
fn get_value(key: &[u8]) -> Result<Option<Value>, StorageError> {
    // Operations that may return StorageError
}
```

### Validating storage boundaries

```rust
storage
    .expire(DataType::Hash, Bytes::from("key"), expire_time)
    .await?;
```

Nimbis selects a typed database before entering the storage layer, so a
same-name key in another type is not a `WRONGTYPE` condition. Malformed rows in
a typed database are reported as decode or data-consistency errors instead.

### Propagating Errors with ?

```rust
fn decode_value(data: &[u8]) -> Result<Value, StorageError> {
    let decoded = decode(data)?; // Converts DecoderError to StorageError
    Ok(decoded)
}
```

### Logging with Error Codes

```rust
match operation() {
    Ok(result) => result,
    Err(e) => {
        error!("Storage error [{}]: {}", e.code(), e);
        return Err(e);
    }
}
```

## Error Code Design Principles

1. **Uniqueness**: Each error variant has a unique code
2. **Hierarchical**: E0xxx for low-level, E1xxx for high-level errors
3. **Nested Codes**: Wrapped errors show full chain (e.g., "E1002:E0001")
4. **Human-readable**: Display messages provide context for debugging
