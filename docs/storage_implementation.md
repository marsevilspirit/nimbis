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
    pub(crate) meta_db: Arc<Db>,
    pub(crate) string_db: Arc<Db>,
    pub(crate) hash_db: Arc<Db>,
    // TODO: add more type db
}
```

It leverages multiple `SlateDB` instances for different data types.
- **Multi-Engine**: Separate DBs for `Meta`, `String`, `Hash`, etc., to avoid key collisions without manual type prefixes.
- **Persistence**: Data is stored using `object_store` via `SlateDB`. By default, it uses the local file system.
- **Concurrency**: `SlateDB` handles underlying concurrency control. The `Storage` struct is cheap to clone (`Arc`'d DBs).

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
}
```

- **Encoding**: 
  - `StringKey` (in `crates/storage/src/string/key.rs`) handles key encoding. No manual prefixes are used as data is isolated in `string_db`.
  - `StringValue` (in `crates/storage/src/string/value.rs`) manages the raw bytes.

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

- **Metadata**: Stored in `meta_db` using `MetaKey` and `HashMetaValue`.
- **Fields**: Stored in `hash_db` using `HashFieldKey` (`user_key` + `len(field)` + `field`).

## Usage

The server initializes the storage in `Storage::open`:

```rust
// Initialize persistent storage at local path
let storage = Storage::open("./nimbis_data").await?;
```

Commands interact with the storage via `Arc<Storage>`.
