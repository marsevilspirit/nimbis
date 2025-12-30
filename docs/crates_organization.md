# Nimbis Crates Organization

Nimbis is organized as a Cargo workspace with multiple focused crates:

## Core Crates

### `command`
The command system implementation, providing the framework for defining and executing Redis-compatible commands.

**Location**: `crates/command/`

**Key Components**:
- `CmdMeta`: Command metadata (name, arity)
- `Cmd` trait: Interface that all commands must implement
- `CmdTable`: Command registry
- `ParsedCmd`: Parsed command structure
- Built-in commands: GET, SET, PING, CONFIG

**Documentation**: See [Command System Implementation](cmd_implementation.md)

### `config`
Configuration management system with derive macros for easy configuration handling.

**Location**: `crates/config/`

**Key Components**:
- `Config` derive macro
- Field setting and getting with type safety
- Immutable field support

**Documentation**: See [Config Crate](config_crate.md)

### `resp`
RESP (REdis Serialization Protocol) parser and encoder implementation.

**Location**: `crates/resp/`

**Key Components**:
- RESP parser
- RESP encoder
- Type definitions for RESP values

**Documentation**: See [Nimbis RESP Design and Usage](nimbis_resp_design_and_usage.md)

### `storage`
Persistent storage layer using SlateDB.

**Location**: `crates/storage/`

**Key Components**:
- `Storage` struct with async `get`/`set` methods
- String key/value encoding
- SlateDB integration

**Documentation**: See [Storage Implementation](storage_implementation.md) and [SlateDB Redis Design](slatedb_redis_design.md)

### `telemetry`
Logging and observability infrastructure.

**Location**: `crates/telemetry/`

**Key Components**:
- Logger initialization
- Tracing setup

### `nimbis`
The main server executable, integrating all the above crates.

**Location**: `crates/nimbis/`

**Key Components**:
- `Server` struct
- TCP connection handling
- Request processing
- Configuration management

**Documentation**: See [Server Design](server_design.md)

## Dependency Graph

```
nimbis
├── command
│   ├── resp
│   ├── storage
│   ├── async-trait
│   ├── bytes
│   └── config
├── resp
├── storage
├── telemetry
└── config

command (standalone)
├── resp
├── storage
├── async-trait
├── bytes
└── config
```

## Adding a New Command

To add a new command to Nimbis:

1. Create a new file in `crates/command/src/your_command.rs`
2. Implement the `Cmd` trait
3. Export it in `crates/command/src/lib.rs`
4. Register it in `crates/command/src/cmd_table.rs`

See [Command System Implementation](cmd_implementation.md) for detailed instructions.

## Running Tests

```bash
# Run all tests
cargo test

# Run tests for a specific crate
cargo test --package command
cargo test --package resp
cargo test --package storage
```

## Building

```bash
# Build all crates
cargo build

# Build release version
cargo build --release
```

## Running the Server

```bash
cargo run --package nimbis
```
