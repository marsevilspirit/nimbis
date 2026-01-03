# Storage Implementation

This document describes the storage layer implementation in Nimbis.

## Overview

Nimbis uses a persistent object storage backed by [SlateDB](https://github.com/slatedb/slatedb). The core abstraction is the concrete `Storage` struct.

## Architecture

### The `Storage` Struct

The core interface is defined by the `Storage` struct in `crates/storage/src/storage.rs`. It provides an asynchronous key-value interface.

```rust
#[derive(Clone)]
pub struct Storage {
    pub(crate) string_db: Arc<Db>,
    pub(crate) hash_db: Arc<Db>,
    // TODO: add more type db
}
```

It leverages multiple `SlateDB` instances:
- **String DB**: Stores actual String values AND Metadata for all other types.
- **Hash DB**: Stores Hash fields.
- **Unified Key Space**: `String DB` uses a type code prefix (e.g. `s`, `h`) in the Value to resolve type collisions.

### String Operations

String-specific operations (`get` and `set`) are implemented in `crates/storage/src/storage_string.rs`.

```rust
impl Storage {
    pub async fn get(
        &self,
        key: Bytes,
    ) -> Result<Option<Bytes>, Box<dyn std::error::Error + Send + Sync>>;

    pub async fn set(
        &self,
        key: Bytes,
        value: Bytes,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    pub async fn expire(&self, key: Bytes, expire_time: u64) -> Result<bool, ...>;
    pub async fn ttl(&self, key: Bytes) -> Result<Option<i64>, ...>;
    pub async fn exists(&self, key: Bytes) -> Result<bool, ...>;
}
```

- **Encoding**: 
  - `StringKey` (in `crates/storage/src/string/key.rs`) handles key encoding. No manual prefixes are used as data is isolated in `string_db`.
  - `StringValue` (in `crates/storage/src/string/value.rs`) manages the raw bytes.

- **Binary Format**:
  - `[DataType::String (1 byte)] [expire_time (8 bytes BE)] [payload (remainder)]`
  - `expire_time` is milliseconds since epoch. `0` means no expiration.

### Hash Operations

Hash operations are implemented in `crates/storage/src/storage_hash.rs`.

```rust
impl Storage {
    pub async fn hset(&self, key: Bytes, field: Bytes, value: Bytes) -> Result<i64, ...>;
    pub async fn hget(&self, key: Bytes, field: Bytes) -> Result<Option<Bytes>, ...>;
    pub async fn hlen(&self, key: Bytes) -> Result<u64, ...>;
    pub async fn hmget(&self, key: Bytes, fields: &[Bytes]) -> Result<Vec<Option<Bytes>>, ...>;
    pub async fn hgetall(&self, key: Bytes) -> Result<Vec<(Bytes, Bytes)>, ...>;
}
```

- **Metadata**: Stored in `string_db` using `MetaKey` and `HashMetaValue` (with type prefix).
- **Binary Format (Metadata)**:
  - `[DataType::Hash (1 byte)] [field_count (8 bytes BE)] [expire_time (8 bytes BE)]`
- **Fields**: Stored in `hash_db` using `HashFieldKey` (`user_key` + `len(field)` as u32 BigEndian + `field`).
- **Strict Metadata Check**: Hash read operations (`hget`, `hmget`, `hgetall`, `hlen`) perform a strict metadata check. If the metadata in `string_db` is missing (due to expiration or key deletion), the command treats the hash as non-existent, even if orphaned fields remain in `hash_db`. This ensures consistent lazy expiration behavior.

### Expiration Trait

To ensure consistent TTL/expiration behavior across different value types, the storage layer implements an `Expirable` trait in `crates/storage/src/expirable.rs`.

#### Trait Interface

```rust
pub trait Expirable {
    // Required methods
    fn expire_time(&self) -> u64;
    fn set_expire_time(&mut self, timestamp: u64);
    
    // Default implementations (can be overridden)
    fn is_expired(&self) -> bool;
    fn expire_at(&mut self, timestamp: u64);
    fn expire_after(&mut self, duration: Duration);
    fn remaining_ttl(&self) -> Option<Duration>;
}
```

#### Implementations

- **StringValue** implements `Expirable` to manage expiration for String type keys.
- **HashMetaValue** implements `Expirable` to manage expiration for Hash type keys.

This design:
- Eliminates code duplication (previously ~54 lines of identical expiration logic)
- Ensures type-safe and consistent expiration behavior
- Makes it easy to add expiration support to future data types (Lists, Sets, etc.)

## Usage

The server initializes the storage in `Storage::open`:

```rust
// Initialize persistent storage at local path
let storage = Storage::open("./nimbis_data").await?;
```

Commands interact with the storage via `Arc<Storage>`.
