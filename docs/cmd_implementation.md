# Command System Implementation and Usage

This document details the implementation architecture and usage patterns for the command system in Nimbis.

## Architecture Overview

The command system allows defining and executing Redis-compatible commands independently. It separates command metadata (immutable definition) from execution logic.

### Core Components

The system is built around the following core components defined in `crates/nimbis/src/cmd/mod.rs`:

1.  **`CmdMeta`**: Contains immutable metadata for a command.
    *   `name`: The command name (e.g., "SET", "GET").
    *   `arity`: The expected number of arguments.
        *   Positive (> 0): Exact number of arguments required.
        *   Negative (< 0): Minimum number of arguments required (absolute value represents the max, but the logic effectively treats it as "up to N" or "variadic" depending on implementation interpretation in validation). *Note: The current `validate_arity` implementation treats negative arity `-N` as allowing up to `N` arguments.*

2.  **`Cmd` Trait**: The interface that all commands must implement.
    *   `meta(&self) -> &CmdMeta`: Returns the command's metadata.
    *   `validate_arity(&self, arg_count: usize) -> Result<(), String>`: Validates if the provided argument count matches the command's arity. Has a default implementation delegating to `CmdMeta`.
    *   `execute(&self, storage: &storage, args: &[String]) -> RespValue`: The main entry point for execution. It handles validation (arity check) automatically before calling `do_cmd`.
    *   `do_cmd(&self, storage: &storage, args: &[String]) -> RespValue`: The actual execution logic of the command. This must be implemented by concrete commands.

3.  **`CMD_TABLE`**: A global, thread-safe registry (`OnceLock<HashMap>`) storing instances of all available commands. The commands are stored as `Arc<dyn Cmd>`.

## Implementing a New Command

To add a new command (e.g., `PING`), follow these steps:

### 1. Define the Command Struct

Create a new file (e.g., `src/cmd/ping.rs`) and define your command struct. It should hold its own `CmdMeta`.

```rust
use crate::cmd::{Cmd, CmdMeta, storage};
use async_trait::async_trait;
use resp::RespValue;

pub struct PingCommand {
    meta: CmdMeta,
}
```

### 2. Implement Creation Logic

Implement a `new` method to initialize the metadata.

```rust
impl PingCommand {
    pub fn new() -> Self {
        Self {
            meta: CmdMeta {
                name: "PING".to_string(),
                arity: -1, // Example: PING accepts 0 or 1 argument
            },
        }
    }
}

impl Default for PingCommand {
    fn default() -> Self {
        Self::new()
    }
}
```

### 3. Implement the `Cmd` Trait

Implement the `Cmd` trait to provide metadata access and execution logic.

```rust
#[async_trait]
impl Cmd for PingCommand {
    fn meta(&self) -> &CmdMeta {
        &self.meta
    }

    async fn do_cmd(&self, _storage: &storage, args: &[String]) -> RespValue {
        if args.is_empty() {
            RespValue::simple_string("PONG")
        } else {
            // PING argument
            RespValue::bulk_string(&args[0])
        }
    }
}
```

### 4. Register the Command

In `src/cmd/mod.rs`, add your new module and register the command in the `init_cmd_table` function.

```rust
// src/cmd/mod.rs

mod ping; // Add module
pub use ping::PingCommand;

// ...

fn init_cmd_table() -> CmdTable {
    let mut table: CmdTable = HashMap::new();

    table.insert("SET".to_string(), Arc::new(SetCommand::new()));
    table.insert("GET".to_string(), Arc::new(GetCommand::new()));
    table.insert("PING".to_string(), Arc::new(PingCommand::new())); // Register

    table
}
```

## Parsing and Dispatch

Commands are parsed from incoming `RespValue` messages into a `ParsedCmd` struct, which extracts the command name and arguments.

```rust
// The command name is automatically uppercased during parsing
let cmd_name = args[0].as_str()?.to_uppercase();
```

Dispatching is handled by looking up the command name in the global `CMD_TABLE` and calling `execute`:

```rust
if let Some(cmd) = get_cmd_table().get(&parsed_cmd.name) {
    let response = cmd.execute(&storage, &parsed_cmd.args).await;
    // ... handling response
} else {
    // Handle unknown command
}
```
