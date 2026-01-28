# Nimbis Server Design & process

This document describes the design principles, architecture, and running process of the Nimbis server.

## 1. Architecture Overview

Nimbis is designed as an asynchronous, multi-threaded Redis-compatible server built using Rust and the Tokio runtime.

### 1.1 Concurrency Model
The server follows a multi-worker "Acceptor-Worker" pattern:
- **Main Task (Acceptor)**: Binds to the TCP port and accepts incoming connections. It dispatches new connections to Workers using a **Round-robin** strategy.
- **Worker Threads**: A fixed number of workers (typically equal to CPU cores). Each worker runs on its own native thread with a dedicated single-threaded Tokio runtime.
- **Client Handling**: Each worker handles multiple client connections using asynchronous tasks within its runtime.

### 1.2 Data Storage
- **Sharded Storage**: Data is sharded across multiple `Storage` instances. Each Worker owns exactly one shard.
- **Isolation**: Each shard has its own SlateDB instance and data directory (`shard-{id}`).
- **Zero Locks**: No cross-shard locks or shared storage handles are used for primary data operations, maximizing vertical scalability.

### 1.3 Command Dispatch
- **Command Table**: Commands are registered in a `CmdTable` which maps command names to their implementations.
- **Dynamic Lookup**: Incoming commands are looked up at runtime and executed via a common `Cmd` trait.
- **Thread Safety**: The `CmdTable` is also wrapped in `Arc` for safe concurrent access.

### 1.4 Protocol
Nimbis speaks the **RESP (REdis Serialization Protocol)**.
- It parses incoming byte streams into RESP types (Strings, Arrays, Integers).
- It executes commands and returns RESP-encoded responses.

For more details on persistent storage, see [Storage Implementation](storage_implementation.md).

---

## 2. Server Implementation Details

The core logic resides in `crates/nimbis/src/server.rs`.

### 2.1 The `Server` Struct
The `Server` struct manages the lifespan of the workers:
```rust
pub struct Server {
    workers: Vec<Worker>,
}
```

### 2.2 Lifecycle

#### Step 1: Configuration Initialization
Before creating the server, the main application initializes the configuration:
```rust
let args = Cli::parse();
telemetry::logger::init(&args.log_level); // Initialize logging/tracing with log level
config::setup(args); // Initialize global config from CLI args
```

The configuration is stored in a thread-safe global state (`SERVER_CONF`) using `OnceLock` and `ArcSwap` for lock-free concurrent access.

**Dynamic Configuration**: The server supports runtime configuration updates via the `CONFIG SET` command. For example, the log level can be changed dynamically:
```
CONFIG SET log_level debug
```

This triggers a callback (`on_log_level_change`) on the configuration object. The `telemetry` crate uses the `server_config!` macro to access the current log level and updates itself via the `reload` handle. See [Config Crate](config_crate.md) for details on the configuration system.

#### Step 2: Server Creation (`new`)
When `Server::new()` is called:
1. Initializes `CmdTable`.
2. Calculates worker count (defaulting to CPU cores).
3. Creates a unique `Storage` instance for each worker (initialized with a shard ID).
4. Spawns `Worker` threads, passing them their respective `Storage` and a common `CmdTable`.

#### Step 3: Server Execution (`run`)
The `run` method is the entry point for the server's execution loop:
1. **Bind**: Creates a `TcpListener` bound to the configured address.
2. **Accept Loop**: Enters an infinite loop waiting for connections.
   - On `listener.accept()` success:
     - Dispatches the `TcpStream` to the next worker in the rotation via an unbounded channel (`WorkerMessage::NewConnection`).
   - On error: Logs the error and continues listening.

---

### 3.Request Flow to Workers
For details on how workers process these requests, parse RESP, and execute commands, refer to the [Worker Design](worker_design.md).

### 3.4 Error Handling
- **Parsing Errors**: Malformed RESP data results in a protocol error sent to the client, and the connection is closed.
- **Command Errors**: Logical errors (e.g., wrong argument count, type mismatch) return a standard Redis error message (`ERR`, `WRONGTYPE`, etc.) without closing the connection.
- **I/O Errors**: Socket read/write errors are propagated; the handler returns and the connection is closed.
