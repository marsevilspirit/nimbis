# Storage Implementation

This document describes the storage layer implementation in Nimbis.

## Overview

Nimbis uses a flexible, trait-based storage system designed to support various backends. The current default implementation is persistent object storage backed by [SlateDB](https://github.com/slatedb/slatedb).

## Architecture

### The `Storage` Trait

The core interface is defined by the `Storage` trait in `crates/storage/src/lib.rs`. It is an `async_trait` ensuring thread safety (`Send + Sync`).

```rust
#[async_trait]
pub trait Storage: Send + Sync {
    /// Get value by key
    /// Returns Option<Bytes> to support zero-copy optimized paths where possible
    async fn get(
        &self,
        key: &str,
    ) -> Result<Option<Bytes>, Box<dyn std::error::Error + Send + Sync>>;

    /// Set value for key
    /// Takes &str for value to minimize allocations
    async fn set(
        &self,
        key: &str,
        value: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
```

### `ObjectStorage` Backend

The primary implementation is `ObjectStorage`, which leverages `SlateDB` for persistent key-value storage.

- **Persistence**: Data is stored using `object_store` via `SlateDB`. By default, it uses the local file system, but can be configured for cloud object stores (S3, GCS, etc.).
- **Type Conversion**: 
  - Keys are converted to bytes for storage.
  - Values are stored as bytes and returned as `bytes::Bytes`.
- **Concurrency**: `SlateDB` handles underlying concurrency control. The `ObjectStorage` struct is cheap to clone (`Arc<Db>`).

```rust
#[derive(Clone)]
pub struct ObjectStorage {
    db: Arc<Db>,
}
```

## Usage

The server initializes the storage in `Server::new`:

```rust
// Initialize persistent storage at local path
let db = ObjectStorage::open("./nimbis_data").await?;
```

Commands interact with the storage via the `Db` type alias (typically `Arc<dyn Storage>`):

```rust
// GET command
let value = db.get(key).await?;

// SET command
db.set(key, value).await?;
```
