# Storage Implementation

This document describes the storage layer implementation in Nimbis.

## Overview

Nimbis uses a persistent object storage backed by [SlateDB](https://github.com/slatedb/slatedb). The core abstraction is the concrete `Storage` struct.

## Architecture

### The `Storage` Struct

The core interface is defined by the `Storage` struct in `crates/storage/src/storage.rs`. It provides a simple, asynchronous key-value interface.

```rust
#[derive(Clone)]
pub struct Storage {
    pub(crate) db: Arc<Db>,
}
```

It leverages `SlateDB` for persistent key-value storage.
- **Persistence**: Data is stored using `object_store` via `SlateDB`. By default, it uses the local file system, but can be configured for cloud object stores.
- **Concurrency**: `SlateDB` handles underlying concurrency control. The `Storage` struct is cheap to clone (`Arc<Db>`).

### String Operations

String-specific operations (`get` and `set`) are implemented as inherent methods on the `Storage` struct, defined in `crates/storage/src/storage_string.rs`.

```rust
impl Storage {
    pub async fn get(
        &self,
        key: &str,
    ) -> Result<Option<Bytes>, Box<dyn std::error::Error + Send + Sync>>;

    pub async fn set(
        &self,
        key: &str,
        value: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
```

- **Encoding**: 
  - Keys are encoded with a type prefix (e.g., 's' + key) using `StringKey`.
  - Values are stored as raw bytes using `StringValue`.

## Usage

The server initializes the storage in `Server::new`:

```rust
// Initialize persistent storage at local path
let db = Storage::open("./nimbis_data").await?;
```

Commands interact with the storage via the `Storage` struct (typically `Arc<Storage>`):

```rust
// GET command
let value = storage.get(key).await?;

// SET command
storage.set(key, value).await?;
```
