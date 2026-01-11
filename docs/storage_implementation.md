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
    pub(crate) list_db: Arc<Db>,
    pub(crate) set_db: Arc<Db>,
    pub(crate) zset_db: Arc<Db>,
}
```

It leverages multiple `SlateDB` instances:
- **String DB**: Stores actual String values AND Metadata for all data types (Hash, List, etc.).
- **Hash DB**: Stores Hash fields exclusively.
- **List DB**: Stores List elements exclusively.
- **Set DB**: Stores Set members exclusively.
- **ZSet DB**: Stores Sorted Set members and score indices exclusively.
- **Isolated Storage**: Each data type has its own database instance for better isolation and performance.

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
    pub async fn hdel(&self, key: Bytes, fields: &[Bytes]) -> Result<i64, ...>;
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

### List Operations

List operations are implemented in `crates/storage/src/storage_list.rs`.

```rust
impl Storage {
    pub async fn lpush(&self, key: Bytes, elements: Vec<Bytes>) -> Result<u64, ...>;
    pub async fn rpush(&self, key: Bytes, elements: Vec<Bytes>) -> Result<u64, ...>;
    pub async fn lpop(&self, key: Bytes, count: Option<usize>) -> Result<Vec<Bytes>, ...>;
    pub async fn rpop(&self, key: Bytes, count: Option<usize>) -> Result<Vec<Bytes>, ...>;
    pub async fn llen(&self, key: Bytes) -> Result<u64, ...>;
    pub async fn lrange(&self, key: Bytes, start: i64, stop: i64) -> Result<Vec<Bytes>, ...>;
}
```

- **Metadata**: Stored in `string_db` using `ListMetaValue` (`len`, `head`, `tail`, `expire_time`).
- **Elements**: Stored in `list_db` using `ListElementKey` (`user_key` + `sequence`).
- **Deque Implementation**: Uses `head` and `tail` pointers to support efficient `push` and `pop` from both ends. `ListMetaValue` tracks the valid range of sequence numbers.

### Set Operations

Set operations are implemented in `crates/storage/src/storage_set.rs`.

```rust
impl Storage {
    pub async fn sadd(&self, key: Bytes, members: Vec<Bytes>) -> Result<u64, ...>;
    pub async fn srem(&self, key: Bytes, members: Vec<Bytes>) -> Result<u64, ...>;
    pub async fn smembers(&self, key: Bytes) -> Result<Vec<Bytes>, ...>;
    pub async fn sismember(&self, key: Bytes, member: Bytes) -> Result<bool, ...>;
    pub async fn scard(&self, key: Bytes) -> Result<u64, ...>;
}
```

- **Metadata**: Stored in `string_db` using `SetMetaValue` (`len`, `expire_time`).
- **Members**: Stored in `set_db` using `SetMemberKey` (`user_key` + `len(member)` + `member`).
- **Efficiency**: `SCARD` works in O(1) by reading metadata. `SMEMBERS` scans a range in `set_db`.

### Sorted Set (ZSet) Operations

Sorted Set operations are implemented in `crates/storage/src/storage_zset.rs`.

```rust
impl Storage {
    pub async fn zadd(&self, key: Bytes, members: Vec<(f64, Bytes)>) -> Result<u64, ...>;
    pub async fn zrange(&self, key: Bytes, start: i64, stop: i64, with_scores: bool) -> Result<Vec<ZRangeMember>, ...>;
    pub async fn zscore(&self, key: Bytes, member: Bytes) -> Result<Option<f64>, ...>;
    pub async fn zrem(&self, key: Bytes, members: Vec<Bytes>) -> Result<u64, ...>;
    pub async fn zcard(&self, key: Bytes) -> Result<u64, ...>;
}
```

- **Metadata**: Stored in `string_db` using `ZSetMetaValue` (`member_count`, `expire_time`).
- **Binary Format (Metadata)**:
  - `[DataType::ZSet (1 byte)] [member_count (8 bytes BE)] [expire_time (8 bytes BE)]`
- **Dual Index Structure**:
  - **MemberKey**: `user_key` + `len(member)` as u32 BigEndian + `member` → stores the score (8 bytes f64 BE)
  - **ScoreKey**: `user_key` + `score` (8 bytes f64 BE) + `member` → empty value (for ordered iteration)
- **Atomic Operations**: Uses `WriteBatch` to ensure atomicity when updating both indices and metadata.
- **Score Encoding**: Scores are encoded as big-endian f64 bytes to maintain correct lexicographic ordering in the key-value store.

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
- **ListMetaValue** implements `Expirable` to manage expiration for List type keys.
- **SetMetaValue** implements `Expirable` to manage expiration for Set type keys.
- **ZSetMetaValue** implements `Expirable` to manage expiration for Sorted Set type keys.

This design:
- Eliminates code duplication (previously ~54 lines of identical expiration logic)
- Ensures type-safe and consistent expiration behavior
- Makes it easy to add expiration support to future data types (Sets, Sorted Sets, etc.)

## Usage and Initialization

### Storage Initialization

The server initializes the storage in `Storage::open`:

```rust
// Initialize persistent storage at local path
let storage = Storage::open("./nimbis_data").await?;
```

This method:
1. Creates a local file system backend using the provided path
2. Initializes separate SlateDB instances for String, Hash, List, Set, and ZSet data
3. Returns an `Arc<Storage>` that can be shared across threads

### Directory Structure

When you call `Storage::open("./nimbis_data")`, it creates the following structure:

```
nimbis_data/
├── string/          # String key-value and metadata storage
├── hash/            # Hash fields storage
├── list/            # List elements storage
├── set/             # Set members storage
└── zset/            # Sorted Set members and score indices
```

Each directory contains SlateDB's internal files (manifests, WAL, SST files, etc.).

### Usage in Commands

Commands interact with the storage via `Arc<Storage>`:

```rust
// Example: String operation
let value = storage.get(key).await?;

// Example: Hash operation
storage.hset(key, field, value).await?;

// Example: List operation
storage.lpush(key, elements).await?;

// Example: ZSet operation
storage.zadd(key, vec![(1.0, member1), (2.0, member2)]).await?;
```

The `Arc<Storage>` is cloned and passed to command handlers, ensuring thread-safe access to all databases.
