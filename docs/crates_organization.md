# Nimbis Crates Organization

Nimbis is organized as a Cargo workspace with multiple focused crates:

## Component Crates

### `macros`
Procedural macros for the configuration system.

**Location**: `crates/macros/`

**Key Components**:
- `OnlineConfig` derive macro
- Attribute parsing for `immutable` and `callback`

### `resp`
RESP (REdis Serialization Protocol) parser and implementation.

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
- `Storage` struct with 5 isolated SlateDB instances (`string_db`, `hash_db`, `list_db`, `set_db`, `zset_db`)
- Per-worker sharded storage architecture (`shard-{id}/` subdirectories)
- Type-specific encoding logic (StringKey, HashFieldKey, etc.)
- SlateDB integration

**Documentation**: See [Storage Implementation](storage_implementation.md) and [SlateDB Redis Design](slatedb_redis_design.md)

### `telemetry`
Logging and observability infrastructure.

**Location**: `crates/telemetry/`

**Key Components**:
- Logger initialization
- Tracing setup

### `nimbis` (Main Crate)
The main executable, integrating all crates, implementing the command system and managing configuration.

**Location**: `crates/nimbis/`

**Key Components**:
- **Configuration Management** (`src/config.rs`): Global `SERVER_CONF`, `server_config!` macro, and dynamic update logic.
- `Server` struct
- TCP connection handling
- **Command System** (`src/cmd/`): Meta, Trait, and concrete command implementations (GET, SET, HSET, etc.)
- Request processing

**Documentation**: See [Server Design](server_design.md), [Config Design](config.md), and [Command System Implementation](cmd_implementation.md)

## Dependency Graph

```
nimbis
├── resp
├── storage
│   ├── bytes
│   ├── slatedb
│   └── object_store
├── telemetry
└── macros (transitive via config)
```


## Adding a New Command

To add a new command to Nimbis:

1. Create a new file in `crates/nimbis/src/cmd/cmd_your_command.rs`
2. Implement the `Cmd` trait
3. Export it in `crates/nimbis/src/cmd/mod.rs`
4. Register it in `crates/nimbis/src/cmd/table.rs`

See [Command System Implementation](cmd_implementation.md) for detailed instructions.

## Running Tests

```bash
# Run unit tests
just test

# Run e2e tests
just e2e-test
```

## Building

```bash
# Build all crates
just build
```

## Running the Server

```bash
just run
```
