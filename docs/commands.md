# Commands

This document summarizes the command framework and the currently implemented Redis-compatible commands.

## Command Framework

Command implementation lives in `nimbis/src/cmd/`.

Core types in `nimbis/src/cmd/mod.rs`:

- `CmdMeta { name, arity, key_spec, kind }`
- `CommandKind { Read, Write, Admin, Local }`
- `KeySpec { None, First, All, Step, Positions }`
- `CmdContext { client_id }`
- `Cmd` trait (`meta`, `plan`, `do_cmd`, `execute`)
- `CommandPlan` and multi-key aggregation types in `nimbis/src/coordinator.rs`
- `ParsedCmd`
- `CmdTable`

`Cmd::execute` performs arity validation first, then calls `do_cmd` with the
storage selected by the dispatcher or worker execution path.
The dispatcher validates arity, asks the command for a `CommandPlan`, and passes
that plan to the multi-key coordinator.

Command metadata is the routing source of truth:

- `CommandKind::Local`: executes inline in the `CommandDispatcher` for the client session
- `CommandKind::Admin`: broadcasts to all workers
- `CommandKind::Read`: hashes extracted keys and defaults to one worker; multi-key reads can override `plan` for scatter-gather
- `CommandKind::Write`: hashes extracted keys, rejects cross-shard writes by default, and is locked by key in the worker execution path
- `KeySpec` defines how keys are extracted from command arguments before routing or locking

## Arity Rules

Nimbis follows Redis-style arity conventions:

- `arity > 0`: exact number of tokens required (including command name)
- `arity < 0`: minimum number of tokens required (including command name)
- validation uses `args.len() + 1`

Examples:

- `GET key` => arity `2`
- `PING [message]` => arity `-1`
- `EXISTS key [key ...]` => arity `-2`

## Supported Commands (Current)

Source of truth: `nimbis/src/cmd/table.rs`.

### Generic

- `PING` (`-1`)
- `HELLO` (`-1`) — supports protocol `2` and `3`
- `DEL` (`-2`)
- `EXISTS` (`-2`)
- `EXPIRE` (`3`)
- `TTL` (`2`)
- `INCR` (`2`)
- `DECR` (`2`)
- `FLUSHDB` (`1`)

### String

- `SET` (`3`)
- `GET` (`2`)
- `MGET` (`-2`)
- `MSET` (`-3`)
- `MSETNX` (`-3`)
- `APPEND` (`3`)

### Hash

- `HSET` (`-4`)
- `HDEL` (`-3`)
- `HGET` (`3`)
- `HLEN` (`2`)
- `HMGET` (`-3`)
- `HGETALL` (`2`)

### List

- `LPUSH` (`-3`)
- `RPUSH` (`-3`)
- `LPOP` (`-2`)
- `RPOP` (`-2`)
- `LLEN` (`2`)
- `LRANGE` (`4`)

### Set

- `SADD` (`-3`)
- `SMEMBERS` (`2`)
- `SUNION` (`-2`)
- `SINTER` (`-2`)
- `SDIFF` (`-2`)
- `SISMEMBER` (`3`)
- `SREM` (`-3`)
- `SCARD` (`2`)

### Sorted Set

- `ZADD` (`-4`)
- `ZRANGE` (`-4`) — by **rank range** (`start stop [WITHSCORES]`)
- `ZSCORE` (`3`)
- `ZREM` (`-3`)
- `ZCARD` (`2`)

### Configuration / Client

- `CONFIG` (`-3`)
  - `CONFIG GET <pattern>`
  - `CONFIG SET <field> <value>`
- `CLIENT` (`-2`)
  - `CLIENT ID`
  - `CLIENT SETNAME <name>`
  - `CLIENT GETNAME`
  - `CLIENT LIST`

## Add a New Command

1. Add `cmd_xxx.rs` under `nimbis/src/cmd/`.
2. Implement `Cmd` for the command struct.
3. Export the module in `nimbis/src/cmd/mod.rs`.
4. Register it in `nimbis/src/cmd/table.rs`.

## Redis Compatibility Notes (Known Gaps)

Nimbis is Redis-compatible for the implemented subset, but does **not** yet implement full Redis semantics.

- `SET` currently documents/implements the basic `SET key value` form only (no `NX|XX|EX|PX|KEEPTTL|GET` options).
- `ZRANGE` supports `start stop [WITHSCORES]` rank mode only; flags such as `BYSCORE`, `BYLEX`, `REV`, and `LIMIT` are not part of this interface.
- `CONFIG` is limited to `GET` and `SET` subcommands.
- `CLIENT` is limited to `ID`, `SETNAME`, `GETNAME`, and `LIST`.
- Transactions (`MULTI`/`EXEC`), pub/sub, scripting, streams, cluster commands, and ACL are not documented as implemented in this command table.

When adding new commands or options, update both `nimbis/src/cmd/table.rs` and this document together.

## Multi-Key Routing Notes

- Multi-key commands declare a `CommandPlan`; the dispatcher does not branch on
  individual command names.
- `DEL key [key ...]` and `EXISTS key [key ...]` use scatter-gather and sum
  shard-local integer responses.
- `MGET key [key ...]` uses scatter-gather and preserves the input key order in
  the final array.
- `MSET key value [key value ...]` uses the multi-key lock coordinator, groups
  writes by shard, and returns `OK` after all shards acknowledge.
- `MSETNX key value [key value ...]` uses the multi-key lock coordinator to keep
  concurrent `MSETNX` and locked `MSET` operations ordered across shards.
- `SUNION`, `SINTER`, and `SDIFF` scatter `SMEMBERS` subrequests and aggregate
  the returned sets.
