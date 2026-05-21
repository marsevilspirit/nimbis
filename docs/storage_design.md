# Storage Design

This document describes the current storage design in `nimbis-storage`.

## Overview

Nimbis uses **five isolated SlateDB instances** for the logical database:

- `string_db`: String payloads and metadata for non-string types
- `hash_db`: Hash fields
- `list_db`: List elements
- `set_db`: Set members
- `zset_db`: Sorted-set indexes

The `Storage` struct is defined in `nimbis-storage/src/storage.rs`:

```rust
#[derive(Clone)]
pub struct Storage {
    pub(crate) string_db: Arc<Db>,
    pub(crate) hash_db: Arc<Db>,
    pub(crate) list_db: Arc<Db>,
    pub(crate) set_db: Arc<Db>,
    pub(crate) zset_db: Arc<Db>,
    locks: Arc<StorageLocks>,
}
```

Each data type has its own database instance for isolation and predictable performance.
`Storage::open(path, shard_id)` and `Storage::open_object_store(url, options, shard_id)`
open all five DBs under either the root path (`None`) or a shard subdirectory (`Some(id)`).
The server opens one shared storage instance with `None`.

## Storage API Locking

`Storage` owns concurrency control through
`nimbis-storage/src/lock.rs`.

The lock state has two layers:

- a database-level `RwLock<()>`
- a fixed striped table of key-level `RwLock<()>` values

Regular key commands acquire a database read lock, hash raw keys into fixed
lock stripes, then acquire those stripes in ascending index order. Read
commands use read locks, write commands use write locks, and any stripe that
contains both read and write keys is treated as a write stripe. This bounds
lock memory regardless of key cardinality while preserving deterministic
multi-key lock ordering.

`FLUSHDB` acquires the database write lock and is mutually exclusive with all
regular key commands.

Lock selection happens inside storage methods, not in command handlers. Public
APIs such as `get`, `set`, `incr`, `hset`, `lrange`, `zadd`, and `flush_all`
acquire the appropriate lock before touching SlateDB. Multi-key APIs such as
`del_many` and `exists_many` acquire the whole stripe set in one storage call
so their lock ordering and deduplication stay centralized.

## Key Encoding

All user keys are length-prefixed (`u16 BE`) to avoid prefix collisions.

### Meta key (in `string_db`)

```text
[len(user_key) (u16 BE)] [user_key]
```

### String value (in `string_db`)

```text
[type (u8)] [raw bytes]
```

> TTL for string keys is maintained by SlateDB TTL metadata (not embedded in the payload bytes).

### Hash metadata (`string_db`)

```text
[type (u8)] [version (u64 BE)] [len (u64 BE)] [expire_time_ms (u64 BE)]
```

### List metadata (`string_db`)

```text
[type (u8)] [version (u64 BE)] [len (u64 BE)] [head (u64 BE)] [tail (u64 BE)] [expire_time_ms (u64 BE)]
```

### Set metadata (`string_db`)

```text
[type (u8)] [version (u64 BE)] [len (u64 BE)] [expire_time_ms (u64 BE)]
```

### ZSet metadata (`string_db`)

```text
[type (u8)] [version (u64 BE)] [len (u64 BE)] [expire_time_ms (u64 BE)]
```

### Collection entry keys

- Hash field key: `[meta_key_prefix] [len(field) (u32 BE)] [field]`
- List element key: `[meta_key_prefix] [seq (u64 BE)]`
- Set member key: `[meta_key_prefix] [len(member) (u32 BE)] [member]`
- ZSet member index key: `[meta_key_prefix] ['M'] [len(member) (u32 BE)] [member]`
- ZSet score index key: `[meta_key_prefix] ['S'] [score (u64 encoded)] [member]`

ZSet score encoding uses bit transforms so lexicographic key order matches numeric order.

## Version + Compaction Strategy

Collection metadata includes a `version`. Collection entry records are written with versioned key prefixes (via `MetaKeyVersion`).

- Read path uses metadata version to determine visible entries.
- Overwrite/delete can advance version and logically invalidate old records.
- `CollectionCompactionFilter` removes stale collection entries during compaction by checking current metadata and type.

This keeps front-path operations simple while cleaning obsolete records asynchronously.

## TTL / Expiration

Expiration for all top-level keys is driven by `string_db` metadata TTL:

- `Storage::meta_put_opts` converts `MetaValue::remaining_ttl()` into SlateDB TTL options.
- `Storage::get_meta` additionally checks `kv.expire_ts`; expired metadata is lazily deleted and treated as missing.
- Collection DB entries do not have independent TTL; they are considered nonexistent once their metadata expires.
- Compaction filters later clean up orphaned collection records.

`TTL` command semantics:

- `> 0`: seconds remaining
- `-1`: key exists without expiration
- `-2`: key does not exist (or already expired)

## Storage Layout

The server's default layout is:

```text
{object_store_url path}/
  string/
  hash/
  list/
  set/
  zset/
```

The storage API still accepts an optional shard ID for tests and lower-level
experiments. When `Some(id)` is provided, files are rooted under
`{object_store_url path}/shard-{id}/`.

## Storage Initialization

The server initializes one shared storage instance from the configured object
store URL and options:

```rust
let storage = Storage::open_object_store(
    "file:nimbis_store",
    std::iter::empty::<(&str, &str)>(),
    None,
).await?;
```

This flow parses the URL/options into an object store backend, then opens the
five SlateDB instances under the configured root.
