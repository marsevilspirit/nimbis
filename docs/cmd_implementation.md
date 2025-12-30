# Command System Implementation and Usage

This document details the implementation architecture and usage patterns for the command system in Nimbis.

## Architecture Overview

The command system allows defining and executing Redis-compatible commands independently. It separates command metadata (immutable definition) from execution logic.

The command system is implemented as a separate crate (`command`) to promote modularity and reusability.

### Core Components

The system is built around the following core components defined in `crates/command/src/lib.rs`:

1.  **`CmdMeta`**: Contains immutable metadata for a command.
    *   `name`: The command name (e.g., "SET", "GET").
    *   `arity`: The expected number of arguments.
        *   Positive (> 0): Exact number of arguments required.
        *   Negative (< 0): Minimum number of arguments required (absolute value represents the minimum).

2.  **`Cmd` Trait**: The interface that all commands must implement.
    *   `meta(&self) -> &CmdMeta`: Returns the command's metadata.
    *   `validate_arity(&self, arg_count: usize) -> Result<(), String>`: Validates if the provided argument count matches the command's arity. Has a default implementation delegating to `CmdMeta`.
    *   `execute(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue`: The main entry point for execution. It handles validation (arity check) automatically before calling `do_cmd`.
    *   `do_cmd(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue`: The actual execution logic of the command. This must be implemented by concrete commands.

3.  **`CmdTable`**: A command registry storing instances of all available commands (defined in `crates/command/src/cmd_table.rs`). The commands are stored as `Arc<dyn Cmd>`.

4.  **`ParsedCmd`**: A structure representing a parsed command with its name and arguments.

## Implementing a New Command

To add a new command (e.g., `PING`), follow these steps:

### 1. Define the Command Struct

Create a new file in the `command` crate (e.g., `crates/command/src/ping.rs`) and define your command struct. It should hold its own `CmdMeta`.

```rust
use std::sync::Arc;

use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use crate::Cmd;
use crate::CmdMeta;

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
                arity: -1, // PING accepts 0 or more arguments
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

In `crates/command/src/lib.rs`, add your new module:

```rust
// crates/command/src/lib.rs

mod ping; // Add module

// Export the command
pub use ping::PingCommand;
```

Then, in `crates/command/src/cmd_table.rs`, register the command in the `CmdTable::new()` function:

```rust
impl CmdTable {
    pub fn new() -> Self {
        let mut inner: HashMap<String, Arc<dyn Cmd>> = HashMap::new();
        inner.insert("SET".to_string(), Arc::new(SetCommand::new()));
        inner.insert("GET".to_string(), Arc::new(GetCommand::new()));
        inner.insert("PING".to_string(), Arc::new(PingCommand::new())); // Register
        inner.insert("CONFIG".to_string(), Arc::new(ConfigCommandGroup::new()));
        Self { inner }
    }
}
```

## Parsing and Dispatch

Commands are parsed from incoming `RespValue` messages into a `ParsedCmd` struct, which extracts the command name and arguments.

```rust
// The command name is automatically uppercased during parsing
let cmd_name = args[0].as_str()?.to_uppercase();
```

Dispatching is handled by looking up the command name in the `CmdTable` and calling `execute`:

```rust
if let Some(cmd) = cmd_table.get_cmd(&parsed_cmd.name) {
    let response = cmd.execute(&storage, &parsed_cmd.args).await;
    // ... handling response
} else {
    // Handle unknown command
}
```

## Module Structure

The `command` crate has the following structure:

```
crates/command/
├── Cargo.toml
└── src/
    ├── lib.rs           # Core types: CmdMeta, Cmd trait, ParsedCmd
    ├── cmd_table.rs     # Command registry
    ├── get.rs           # GET command
    ├── set.rs           # SET command
    ├── ping.rs          # PING command
    └── group_config.rs  # CONFIG command group
```

