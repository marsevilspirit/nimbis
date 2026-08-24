# Commands

This document summarizes the command framework and the currently implemented
Nimbis commands. Nimbis intentionally diverges from Redis for typed key
lifecycle commands so they can route to one physical database without type
discovery.

## Command Framework

Command implementation lives in `nimbis/src/cmd/`.

Core types in `nimbis/src/cmd/mod.rs`:

- `CmdMeta { name, arity }`
- `CmdContext { client_id }`
- `Cmd` trait (`meta`, `do_cmd`, `execute`)
- `ParsedCmd`
- `CmdTable`

`Cmd::execute` performs arity validation first, then calls `do_cmd`.

## Arity Rules

Nimbis follows Redis-style arity conventions:

- `arity > 0`: exact number of tokens required (including command name)
- `arity < 0`: minimum number of tokens required (including command name)
- validation uses `args.len() + 1`

Examples:

- `GET key` => arity `2`
- `PING [message]` => arity `-1`
- `EXISTS STRING key [key ...]` => arity `-3`

## Supported Commands (Current)

Source of truth: `nimbis/src/cmd/table.rs`.

Nimbis extends Redis key semantics by giving each data type an independent
namespace. The same raw key may simultaneously hold a String, Hash, List, Set,
and ZSet; type-specific commands do not overwrite or reject the other types.
`DEL`, `EXISTS`, `EXPIRE`, and `TTL` require a type selector as their first
argument. The selector is case-insensitive and must be one of `STRING`, `HASH`,
`LIST`, `SET`, or `ZSET`. Each command touches only that type's database; there
is no cross-database type discovery or fallback.

### Generic

- `PING` (`-1`)
- `HELLO` (`-1`) — supports protocol `2` and `3`
- `DEL <TYPE> key [key ...]` (`-3`)
- `EXISTS <TYPE> key [key ...]` (`-3`)
- `EXPIRE <TYPE> key seconds` (`4`)
- `TTL <TYPE> key` (`3`)
- `INCR` (`2`)
- `DECR` (`2`)
- `FLUSHDB` (`1`)

### String

- `SET` (`3`)
- `GET` (`2`)
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

## Benchmark Alignment

The `full` redis-benchmark profile in `xtask/src/redis_benchmark.rs` should
cover this implemented command table. `FLUSHDB` is the exception: it is used for
benchmark setup and cleanup, not throughput comparison.

The `comparison` redis-benchmark profile is intentionally smaller so CI can
compare PR and main branch performance across a stable command subset. It must
still contain only commands listed in this document.

## Add a New Command

1. Add `cmd_xxx.rs` under `nimbis/src/cmd/`.
2. Implement `Cmd` for the command struct.
3. Export the module in `nimbis/src/cmd/mod.rs`.
4. Register it in `nimbis/src/cmd/table.rs`.
5. Update this document, `docs/redis-benchmark.md`, and the benchmark profiles
   in `xtask/src/redis_benchmark.rs` together.

## Redis Compatibility Notes (Known Gaps)

Nimbis is Redis-compatible for the implemented subset, but does **not** yet implement full Redis semantics.

- Unlike Redis's single global key type, Nimbis uses independent typed
  namespaces, so one raw key can hold values of several types concurrently.
- Redis lifecycle command shapes such as `DEL key` and `TTL key` are rejected.
  Clients must send the explicit Nimbis type selector; for example,
  `DEL HASH users` or `TTL ZSET leaderboard`. High-level Redis client helpers
  for these four commands generally cannot express this syntax, so use their
  raw-command API.
- `SET` currently documents/implements the basic `SET key value` form only (no `NX|XX|EX|PX|KEEPTTL|GET` options).
- `ZRANGE` supports `start stop [WITHSCORES]` rank mode only; flags such as `BYSCORE`, `BYLEX`, `REV`, and `LIMIT` are not part of this interface.
- `CONFIG` is limited to `GET` and `SET` subcommands.
- `CLIENT` is limited to `ID`, `SETNAME`, `GETNAME`, and `LIST`.
- Multi-key string helpers like `MGET`/`MSET`, transactions (`MULTI`/`EXEC`), pub/sub, scripting, streams, cluster commands, and ACL are not documented as implemented in this command table.

When adding new commands or options, update `nimbis/src/cmd/table.rs`, this
document, and the benchmark documentation/profile lists together.
