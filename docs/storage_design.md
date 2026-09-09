# Storage Design

This document describes the current storage design in `nimbis-storage`.

## Overview

Nimbis uses **five isolated SlateDB instances** for the logical database:

- `string_db`: String values
- `hash_db`: Hash metadata and fields
- `list_db`: List metadata and elements
- `set_db`: Set metadata and members
- `zset_db`: Sorted-set metadata and both indexes

The `Storage` struct is defined in `nimbis-storage/src/storage.rs`:

```rust
#[derive(Clone)]
pub struct Storage {
    pub(crate) string_db: TypedDb<StringValue>,
    pub(crate) hash_db: TypedDb<HashMetaValue>,
    pub(crate) list_db: TypedDb<ListMetaValue>,
    pub(crate) set_db: TypedDb<SetMetaValue>,
    pub(crate) zset_db: TypedDb<ZSetMetaValue>,
    locks: Arc<StorageLocks>,
}
```

`TypedDb<V>` owns the physical `Arc<Db>` and is the only metadata read/commit
authority for `V`. Collection mutations pass their sub-key batch and metadata
transition to `TypedDb::commit`, which emits one SlateDB `WriteBatch`.

Each data type has its own database instance for isolation and predictable performance.
The same raw user key may exist independently in multiple type databases. Type-specific
commands only access their own database, so, for example, `SET k v` and `HSET k f v`
can coexist without a cross-type `WRONGTYPE` lookup.
`Storage::open(path, shard_id)` and
`Storage::open_object_store(url, options, shard_id, block_cache_capacity_bytes)`
open all five DBs under either the root path (`None`) or a shard subdirectory (`Some(id)`).
The server opens one shared storage instance with `None`.

## Acknowledgment and Recovery Contract

`SET key value` returns `OK` after the write is accepted into SlateDB's in-memory
WAL and MemTable. It does **not** wait for object-storage persistence. A following
`GET` can see the value immediately, but that read does not prove durability.

The call chain is `SetCmd::do_cmd` → `Storage::set` → `TypedDb::store` →
`Db::put_with_options`. The typed write path drops the returned `WriteHandle`.
This follows the API in the SlateDB revision pinned by `Cargo.lock`,
[`4919857e75b30f63ce2005c8d4a64786de6c553d`](https://github.com/slatedb/slatedb/blob/4919857e75b30f63ce2005c8d4a64786de6c553d/slatedb/src/db.rs):
`WriteHandle::await_durable()` waits for that write's durability, and
`Db::flush()` flushes pending WAL writes. Nimbis does not currently expose either
operation as a Redis command or a per-write durability option. `FLUSHDB` deletes
keys; it is not a persistence barrier. The `save` and `appendonly` configuration
fields do not select a SlateDB durability mode or enable Redis RDB/AOF files.

SlateDB's current default flush interval is 100 ms. This is a scheduling target,
not a maximum loss window: object-store latency, errors, and backpressure can
extend it. A successful ACK can precede a later flush failure; such an ACK cannot
be revoked. Before an ACK, a detected write error is returned as a command error.

| Boundary | What is guaranteed |
| --- | --- |
| `SET` returns `OK` | The value is visible in memory; it may still be absent after a process crash. |
| SlateDB reports the write's sequence durable | It is persisted through the configured object store and can be recovered after process exit. |
| Successful `Storage::close()` after stopping writers | All five DBs have completed close and their final flush; errors from any DB are returned. |
| Successful server `SIGINT` / Unix `SIGTERM` shutdown | The listener has stopped, client tasks have been cancelled and joined, and storage close has succeeded before exit. |
| Abrupt process exit / `SIGKILL` | No close is run. Recovery keeps durable writes; recently acknowledged writes may be lost. |

Commands interrupted before their response have an unknown outcome and may have
executed. Client tasks cannot continue writing concurrently with the server's
final storage close. Lower-level users of `Storage::close()` must likewise stop
their writers first. Closing all five DBs is not a cross-type transaction. A
failed DB must not cancel sibling closes or make the server report a successful
shutdown: SlateDB itself can return success when closing an already failed DB
without flushing, so Nimbis also checks the DB's close reason.

Recovery guarantees remain bounded by the backend. Nimbis creates the local
`file:` backend using `LocalFileSystem::new_with_prefix`; object_store 0.14.1
defaults to `fsync = false`. Local tests therefore establish **process-crash
recovery**, not machine power-loss durability. Remote object-store acknowledgments
have the guarantees of that service. No local process test establishes network
partition recovery, cloud-service durability, or power-loss safety.

### Runnable recovery checks

```sh
cargo test -p nimbis-storage --lib recovery
cargo test -p nimbis --test test_command test_process_recovery
```

The storage check starts the test executable as a separate writer process using
a temporary local object store. It disables only that test DB's periodic flush,
checks its durable sequence, and verifies both the pre-flush and post-flush
results after a real `process::exit` without destructors. A valid older value is
flushed before the overwrite, so the pre-flush crash case must recover the older
value. A later `WriteHandle` supplies an explicit durability barrier. Separate
cases exercise clean close, a WAL write held behind a synchronization barrier,
WAL I/O failure during close, and a failed WAL background task. Test-only
SlateDB failpoints inject those conditions; no production option is added.
Sibling list data and DB close status verify that one failed close does not
cancel the others. These tests do not use elapsed time as durability evidence.

The Unix server check runs the real `nimbis` binary, writes through RESP, leaves
a client connected, sends `SIGINT` or `SIGTERM`, waits for successful exit, and
restarts the same object-store root. It checks all five data types, then uses
`SIGKILL` and checks recovery of the established durable value. The most recent
ordinary ACK is permitted either outcome; the deterministic storage check above
establishes the exact durable/pending distinction. The subprocess is reaped and
timeouts bound startup and shutdown. Windows does not run the Unix signal check.

### Redis comparison boundary

Report Nimbis results as **asynchronous WAL acknowledgment throughput**. Record
the backend, cache, CPU allocation, client concurrency, pipeline, value sizes,
and actual Redis persistence settings alongside every comparison. Redis without
persistence, Redis AOF `everysec`, and Redis AOF `always` have different
acknowledgment and synchronization costs; none is an implicit durability match
for Nimbis. Consult the [Redis persistence documentation](https://redis.io/docs/latest/operate/oss_and_stack/management/persistence/)
for those policies. Comparing `SET` throughput alone does not establish equal
durability, and Nimbis `SET OK` must not be labeled synchronous-durable throughput.

## Storage API Locking

`Storage` owns concurrency control through
`nimbis-storage/src/lock.rs`.

The lock state has two layers:

- a database-level `RwLock<()>`
- one fixed striped key-lock table for each data type

Regular key commands acquire a database read lock, map `(data_type, raw_key)`
into a type-local stripe, then acquire the resulting `(type, stripe)` pairs in
ascending order. Same-name keys in different typed namespaces do not block one
another. Read
commands use read locks, write commands use write locks, and any stripe that
contains both read and write keys is treated as a write stripe. This bounds
lock memory regardless of key cardinality while preserving deterministic
multi-key lock ordering.

`FLUSHDB` acquires the database write lock and is mutually exclusive with all
regular key commands.

Lock selection happens inside storage methods, not in command handlers. Public
APIs such as `get`, `set`, `incr`, `hset`, `lrange`, `zadd`, and `flush_all`
acquire the appropriate lock before touching SlateDB. Multi-key APIs such as
`del(data_type, keys)` and `exists_many(data_type, keys)` acquire the whole
stripe set in one storage call so their lock ordering and deduplication stay
centralized.

## Key Encoding

All user keys are length-prefixed (`u16 BE`) to avoid prefix collisions.
SlateDB limits the complete encoded key to 65,535 bytes. `TopLevelKey` validates
top-level keys, and each collection codec validates its complete key—including
field/member/index suffixes—before allocating or writing. A top-level user key
can therefore contain at most 65,533 bytes; collection keys have a smaller
effective user-key limit according to their suffix.

### Top-level key

```text
[len(user_key) (u16 BE)] [user_key]
```

### String value (in `string_db`)

```text
[type (u8)] [raw bytes]
```

> TTL for string keys is maintained by SlateDB TTL metadata (not embedded in the payload bytes).

The exact top-level key is stored in the database for its type. For String it
contains the value; for a collection it contains the collection metadata below.

### Hash metadata (`hash_db`)

```text
[type (u8)] [version (u64 BE)] [len (u64 BE)] [expire_time_ms (u64 BE)]
```

### List metadata (`list_db`)

```text
[type (u8)] [version (u64 BE)] [len (u64 BE)] [head (u64 BE)] [tail (u64 BE)] [expire_time_ms (u64 BE)]
```

### Set metadata (`set_db`)

```text
[type (u8)] [version (u64 BE)] [len (u64 BE)] [expire_time_ms (u64 BE)]
```

### ZSet metadata (`zset_db`)

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
List sequence numbers use big-endian encoding for the same reason: a logical
`LRANGE` maps to one inclusive key interval. After loading metadata, the read
path scans that bounded interval once and filters entries against the current
collection version.

`LPOP` and `RPOP` keep a point read when popping one element. Count pops read
their bounded interval in one scan and verify every expected element key and
collection version before committing deletes and metadata in one batch. `RPOP`
reverses the scanned values to preserve tail-first response order. A missing or
stale element fails the whole command before any mutation is committed. POP scans
cache fetched blocks, retaining the point-read policy for subsequent pops from
the same block.

## Version + Compaction Strategy

Collection metadata includes a `version`. The first metadata row and all initial
sub-keys are committed in one SlateDB `WriteBatch`. The encoded initial version
is `0`, meaning "resolve this generation to the metadata row's commit sequence";
SlateDB assigns the same sequence to every row in that batch.

- Read path uses metadata version to determine visible entries.
- Runtime collection mutations update sub-keys and metadata in one batch in the
  corresponding type DB.
- Delete/recreate advances the effective generation and logically invalidates old records.
- `CollectionCompactionFilter` performs no point reads. It only reclaims stale
  rows when a safe metadata cutoff is present in the same ordered compaction
  input; otherwise it fails open and keeps data.

This keeps the foreground path atomic and avoids a compactor reading the DB it is
currently compacting. Because SlateDB may remove a metadata tombstone before the
custom filter observes it, physical orphan cleanup is opportunistic; logical
visibility remains correct, but some stale sub-keys can remain until a future
dedicated GC/fence format is implemented.

## TTL / Expiration

Expiration for every top-level value is driven by the metadata/value row in its
own typed database:

- `metadata_put_options` converts the value's absolute expiration through the
  shared expiration helper into SlateDB TTL options; callers reach it through
  the typed commit path.
- Collection metadata embeds an absolute expiration timestamp, which is the
  logical source of truth. String expiration uses the SlateDB row TTL because
  String values do not have an embedded metadata timestamp.
- Logical deadlines are capped at `253402214399999` milliseconds since Unix
  epoch. This practical ceiling leaves one day of signed-timestamp headroom for
  SlateDB's later `ExpireAfter` clock read and prevents overflow.
- The current SlateDB API accepts a relative `ExpireAfter` duration rather than
  an absolute deadline. Collection reads therefore enforce the embedded
  deadline even if a row-TTL rewrite or restart introduces a few milliseconds
  of scheduling drift.
- Collection sub-keys do not have independent TTL; they are considered
  nonexistent once their local metadata expires.
- Rewriting collection TTL resolves a pending `version=0` first, so changing TTL
  never changes the collection generation.

Key lifecycle commands require an explicit type selector. `DEL`, `EXISTS`,
`EXPIRE`, and `TTL` resolve that selector once and access only the corresponding
SlateDB instance. A same-name value in another type database is neither read nor
modified.

`DEL <TYPE> key [key ...]` restricts every key in the command to one selected
type database. Existing top-level rows are deleted in one SlateDB `WriteBatch`,
so multi-key deletion has one atomic commit within that database. Collection
fields/elements from an older generation become unreachable as soon as their
local metadata row is deleted and are reclaimed by the compaction filter.

`EXPIRE <TYPE> key seconds` rewrites only the selected type's top-level row.
`EXISTS <TYPE> ...` and `TTL <TYPE> ...` likewise perform no cross-database
probe. This deliberately trades Redis wire compatibility for deterministic
single-database cost and avoids cross-DB transaction semantics entirely.

`TTL <TYPE> key` command semantics for the selected namespace:

- `>= 0`: non-negative seconds remaining (sub-second TTLs round down to `0`)
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
    nimbis_storage::DEFAULT_BLOCK_CACHE_CAPACITY_BYTES,
).await?;
```

This flow parses the URL/options into an object store backend, then opens the
five SlateDB instances under the configured root. Before serving traffic it calls
the private `layout_migration::ensure_current_layout` boundary, which runs an
idempotent, durable migration for the legacy layout: collection metadata
is copied to its typed DB, verified, and only then removed from `string_db`.
Migration scans use bounded 64-row cursor windows so startup memory is bounded.
After every typed migration and source cleanup succeeds, Nimbis writes the root
layout marker `.nimbis` with `nimbis-layout:type-local-metadata:v1`; subsequent
opens skip the full legacy scan. If startup is interrupted before that marker is
written, replay is safe and completes the remaining verified cleanup.
