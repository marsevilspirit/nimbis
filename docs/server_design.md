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

### 1.3 Build Info and Startup Banner
The server includes a comprehensive build information system that executes at compile time via `build.rs`:
- **Git Metadata**: Includes short commit hash, current branch, and a "dirty" flag if there are uncommitted changes.
- **Environment Information**: Captures the build date, Rust compiler version, and target platform (arch/OS).
- **Startup Banner**: Upon startup, the server displays an ASCII logo and these build details to ensure observability of the binary's origin.

### 1.4 Command Dispatch
- **Command Table**: Commands are registered in a `CmdTable` which maps command names to their implementations.
- **Dynamic Lookup**: Incoming commands are looked up at runtime and executed via a common `Cmd` trait.
- **Thread Safety**: The `CmdTable` is also wrapped in `Arc` for safe concurrent access.

### 1.5 Protocol
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

#### Step 0: Build-time environment setup
The `crates/nimbis/build.rs` script runs during compilation to generate environment variables (`NIMBIS_GIT_HASH`, `NIMBIS_BUILD_DATE`, etc.) that are later embedded into the binary using the `env!` macro.

#### Step 1: Configuration Initialization and Logo
Before creating the server, the main application initializes the configuration and displays the startup banner:
```rust
use nimbis::config::{Cli, Parser};
use nimbis::logo;

let args = Cli::parse();
if let Err(e) = nimbis::config::setup(args) {
    log::error!("Failed to load configuration: {}", e);
    std::process::exit(1);
}

logo::show_logo(); // Displays ASCII logo and detailed build information
```

The configuration is stored in a thread-safe global state (`SERVER_CONF`) using `OnceLock` and `ArcSwap` for lock-free concurrent access.

**Dynamic Configuration**: The server supports runtime configuration updates via the `CONFIG SET` command. For example, the log level can be changed dynamically:
```
CONFIG SET log_level debug
```

This triggers a callback (`on_log_level_change`) on the configuration object. The `telemetry` crate uses the `server_config!` macro (exported at the crate root of `nimbis`) to access the current log level and updates itself via the `reload` handle. See [Config Design](config.md) for details on the configuration system.

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
