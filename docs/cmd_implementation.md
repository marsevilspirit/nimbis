# Command System Implementation and Usage

This document details the implementation architecture and usage patterns for the command system in Nimbis.

## Architecture Overview

The command system allows defining and executing Redis-compatible commands independently. It separates command metadata (immutable definition) from execution logic.

The command system is integrated into the `nimbis` crate within the `cmd` module (`crates/nimbis/src/cmd/`).

### Core Components

The system is built around the following core components defined in `crates/nimbis/src/cmd/mod.rs`:

1.  **`CmdMeta`**: Contains immutable metadata for a command.
    *   `name`: The command name (e.g., "SET", "GET").
    *   `arity`: The expected number of arguments (including the command name).
        *   Positive (> 0): Exact number of arguments required.
        *   Negative (< 0): Minimum number of arguments required (absolute value represents the minimum).

2.  **`Cmd` Trait**: The interface that all commands must implement.
    *   `meta(&self) -> &CmdMeta`: Returns the command's metadata.
    *   `execute(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue`: The main entry point for execution. It handles validation (arity check via `validate_arity(args.len() + 1)`) automatically before calling `do_cmd`.
    *   `do_cmd(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue`: The actual execution logic of the command. This must be implemented by concrete commands.

3.  **`CmdTable`**: A command registry storing instances of all available commands (defined in `crates/nimbis/src/cmd/table.rs`). The commands are stored as `Arc<dyn Cmd>`.

4.  **`ParsedCmd`**: A structure representing a parsed command with its name and arguments.

## Implementing a New Command

To add a new command (e.g., `PING`), follow these steps:

### 1. Define the Command Struct

Create a new file in the `cmd` module (e.g., `crates/nimbis/src/cmd/cmd_ping.rs`) and define your command struct. It should hold its own `CmdMeta`.

```rust
use std::sync::Arc;

use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct PingCmd {
    meta: CmdMeta,
}
```

### 2. Implement Creation Logic

Implement a `new` method to initialize the metadata and provide a `Default` implementation.

```rust
impl PingCmd {
    pub fn new() -> Self {
        Self {
            meta: CmdMeta {
                name: "PING".to_string(),
                arity: -1, // PING accepts 0 or more arguments (total args >= 1 including cmd)
            },
        }
    }
}

impl Default for PingCmd {
    fn default() -> Self {
        Self::new()
    }
}
```

### 3. Implement the `Cmd` Trait

Implement the `Cmd` trait to provide metadata access and execution logic.

```rust
#[async_trait]
impl Cmd for PingCmd {
    fn meta(&self) -> &CmdMeta {
        &self.meta
    }

    async fn do_cmd(&self, _storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue {
        if args.is_empty() {
            RespValue::simple_string("PONG")
        } else {
            // Echo back the first argument
            RespValue::bulk_string(args[0].clone())
        }
    }
}
```

### 4. Register the Command

In `crates/nimbis/src/cmd/mod.rs`, add your new module and export it:

```rust
// crates/nimbis/src/cmd/mod.rs

mod cmd_ping;
pub use cmd_ping::PingCmd;
```

Then, in `crates/nimbis/src/cmd/table.rs`, register the command in the `CmdTable::new()` function:

```rust
impl CmdTable {
    pub fn new() -> Self {
        let mut inner: HashMap<String, Arc<dyn Cmd>> = HashMap::new();
        inner.insert("PING".to_string(), Arc::new(PingCmd::new()));
        // ...
        Self { inner }
    }
}
```

## Supported Redis Commands

The following table lists the currently implemented Redis commands and their status.

| Category    | Command      | Arity | Description                                         |
| :---------- | :----------- | :---- | :-------------------------------------------------- |
| **Generic** | `PING`       | `-1`  | Ping the server (optionally with a message).        |
| **Generic** | `DEL`        | `-2`  | Delete one or more keys.                            |
| **Generic** | `EXISTS`     | `-2`  | Check if one or more keys exist.                    |
| **Generic** | `EXPIRE`     | `3`   | Set a timeout on key.                               |
| **Generic** | `TTL`        | `2`   | Get the time to live for a key in seconds.          |
| **String**  | `SET`        | `3`   | Set the string value of a key.                      |
| **String**  | `GET`        | `2`   | Get the value of a key.                             |
| **Hash**    | `HSET`       | `-4`  | Sets field(s) in the hash.                          |
| **Hash**    | `HGET`       | `3`   | Returns the value of a field in the hash.           |
| **Hash**    | `HLEN`       | `2`   | Returns the number of fields in the hash.           |
| **Hash**    | `HMGET`      | `-3`  | Returns the values of specified fields in the hash. |
| **Hash**    | `HGETALL`    | `2`   | Returns all fields and values in the hash.          |
| **Hash**    | `HDEL`       | `-3`  | Delete one or more fields from a hash.              |
| **List**    | `LPUSH`      | `-3`  | Prepend one or multiple elements to a list.         |
| **List**    | `RPUSH`      | `-3`  | Append one or multiple elements to a list.          |
| **List**    | `LPOP`       | `-2`  | Remove and get the first element in a list.         |
| **List**    | `RPOP`       | `-2`  | Remove and get the last element in a list.          |
| **List**    | `LLEN`       | `2`   | Get the length of a list.                           |
| **List**    | `LRANGE`     | `4`   | Get a range of elements from a list.                |
| **Set**     | `SADD`       | `-3`  | Add one or more members to a set.                   |
| **Set**     | `SREM`       | `-3`  | Remove one or more members from a set.              |
| **Set**     | `SMEMBERS`   | `2`   | Get all members in a set.                           |
| **Set**     | `SISMEMBER`  | `3`   | Check if a member is in a set.                      |
| **Set**     | `SCARD`      | `2`   | Get the number of members in a set.                 |
| **ZSet**    | `ZADD`       | `-4`  | Add members with scores to a sorted set.            |
| **ZSet**    | `ZRANGE`     | `-4`  | Get members in a sorted set by score range.         |
| **ZSet**    | `ZSCORE`     | `3`   | Get the score of a member in a sorted set.          |
| **ZSet**    | `ZREM`       | `-3`  | Remove members from a sorted set.                   |
| **ZSet**    | `ZCARD`      | `2`   | Get the number of members in a sorted set.          |
| **Config**  | `CONFIG GET` | `-3`  | Get the value of a configuration parameter.         |
| **Config**  | `CONFIG SET` | `4`   | Set a configuration parameter to a given value.     |
| **Generic** | `FLUSHDB`    | `1`   | Remove all keys from the current database.          |

## Parsing and Dispatch

Commands are parsed from incoming `RespValue` messages into a `ParsedCmd` struct, which extracts the command name and arguments.

Dispatching is handled in `handle_client` (`crates/nimbis/src/server.rs`) by looking up the command name in the `CmdTable` and calling `execute`:

```rust
if let Some(cmd) = cmd_table.inner.get(&parsed_cmd.name) {
    let response = cmd.execute(&storage, &parsed_cmd.args).await;
    // ... handling response
} else {
    // Handle unknown command
}
```

## Module Structure

The command system in `crates/nimbis/src/cmd/` has the following structure:

```
crates/nimbis/src/cmd/
├── cmd_get.rs           # GET command
├── cmd_set.rs           # SET command
├── cmd_ping.rs          # PING command
├── cmd_hset.rs          # HSET command
├── cmd_hdel.rs          # HDEL command
├── cmd_lpush.rs         # LPUSH command
├── cmd_rpush.rs         # RPUSH command
├── cmd_flushdb.rs       # FLUSHDB command
├── ...                  # Other commands
├── group_cmd_config.rs  # CONFIG command group
├── mod.rs               # Core types: CmdMeta, Cmd trait, ParsedCmd
└── table.rs             # Command registry
```
