# Nimbis Server Design

This document describes the current Nimbis server architecture.

## Architecture Overview

Nimbis runs on a single Tokio multi-thread runtime. The listener accepts TCP
connections and spawns one async task per client connection on that runtime.
The runtime thread count is configured by `runtime_threads`.

All client tasks share:

- one `Arc<Storage>` opened from the configured object store root
- one `Arc<CmdTable>` for command lookup
- one global `ClientSessions` registry for `CLIENT` command state

The server no longer shards data per worker. All commands operate on the same
logical database view. Command concurrency is controlled by the storage-owned
locking state inside `Storage`.

## Startup Lifecycle

`nimbis/src/main.rs` is synchronous at the top level:

1. Parse CLI arguments.
2. Load configuration and initialize telemetry.
3. Read `runtime_threads` from `SERVER_CONF`.
4. Build a Tokio multi-thread runtime with that thread count.
5. Run `Server::new()`, `Server::run()`, and shutdown signal handling inside the runtime.
6. Flush telemetry before process exit.

## Server Lifecycle

`Server::new()` initializes shared process state:

1. Create and register `ClientSessions`.
2. Create the command table.
3. Open a single `Storage` with `Storage::open_object_store(..., None)`.

`Server::run()` binds to `host:port`, accepts connections, and spawns a
`ClientConnection` task for each accepted socket.

## Command Execution

Each `ClientConnection` owns a RESP parser and a socket. For every read:

1. Parse all complete RESP commands currently in the buffer.
2. Convert each RESP array into `ParsedCmd`.
3. Execute commands in parse order.
4. Write responses in the same order.

This preserves Redis pipeline response ordering without inter-worker channels.

Command execution follows this order:

1. Look up the command in `CmdTable`.
2. Validate arity.
3. Execute the command against shared `Storage`.
4. Let each storage API acquire and release its own read/write/global lock.

## Locking Model

Storage owns the command-locking state in `nimbis-storage/src/lock.rs`. It has
two layers:

- a database-level `RwLock<()>`
- a map of per-key `RwLock<()>` values

Regular key commands acquire the database read lock, then acquire per-key locks
in sorted raw-byte order. Read commands use read locks, write commands use
write locks, and any key that appears in both read and write sets is treated as
a write key.

`FLUSHDB` acquires the database write lock, making it mutually exclusive with
all regular key commands.

Lock selection is kept inside storage methods rather than command handlers:
for example `Storage::get` acquires a per-key read lock, `Storage::set` and
`Storage::incr` acquire per-key write locks, `Storage::del_many` and
`Storage::exists_many` acquire their full multi-key lock set before iterating,
and `Storage::flush_all` acquires the database write lock.

This design keeps multi-key commands local to one storage view and avoids
scatter-gather routing.

## Error Handling

| Scenario | Handling |
| --- | --- |
| Connection reset | Log at debug level and close the client task |
| Incomplete RESP data | Keep buffered bytes and wait for more data |
| Malformed RESP | Send protocol error and close the connection |
| Unknown command | Return `ERR unknown command` |
| Command/storage error | Return the command-specific Redis error response |

## Source Map

| File | Responsibility |
| --- | --- |
| `nimbis/src/main.rs` | Process entrypoint and Tokio runtime creation |
| `nimbis/src/server.rs` | Listener, shared server state, client task spawning |
| `nimbis/src/client.rs` | RESP parsing, pipeline ordering, command execution |
| `nimbis-storage/src/lock.rs` | Storage-owned database and per-key command locking |
| `nimbis/src/cmd/` | Command definitions and storage API calls |
