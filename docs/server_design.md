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

For more details on persistent storage, see [Storage Design](storage_design.md).

---

## 2. Server Implementation Details

The core logic resides in `nimbis/src/server.rs`.

### 2.1 The `Server` Struct
The `Server` struct manages the lifespan of the workers:
```rust
pub struct Server {
    workers: Vec<Work
}
```

### 2.2 Lifecycle

#### Step 0: Build-time environment setup
The `nimbis/build.rs` script runs during compilation to generate environment variables (`NIMBIS_GIT_HASH`, `NIMBIS_BUILD_DATE`, etc.) that are later embedded into the binary using the `env!` macro.

#### Step 1: Configuration Initialization and Logo
Before creating the server, the main application initializes the configuration and displays the startup banner:
```rust
use nimbis::config::{Cli, Parser};
use nimbis::logo;

let args = Cli::parse();
let telemetry_manager = match nimbis::config::setup(args) {
    Ok(manager) => manager,
    Err(e) => {
        log::error!("Failed to load configuration: {}", e);
        std::process::exit(1);
    }
};

logo::show_logo(); // Displays ASCII logo and detailed build information
```

The configuration is stored in a thread-safe global state (`SERVER_CONF`) using `OnceLock` and `ArcSwap` for lock-free concurrent access. The returned telemetry manager owns telemetry lifecycle state, including logger reload state and trace flushing before shutdown.

**Dynamic Configuration**: The server supports runtime configuration updates via the `CONFIG SET` command. For example, the log level can be changed dynamically:
```
CONFIG SET log_level debug
```

This triggers the configuration object's callback, which uses the telemetry manager registered during startup to update the logger reload handle. See [Config Design](config.md) for details on the configuration system.

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

## 3. Worker Design & Implementation

### 3.1 Overview

The Worker subsystem is responsible for handling client commands and managing connections. It implements a multi-worker architecture with consistent hashing for request routing.

#### 3.1.1 Key Design Goals

- **Multi-core Utilization**: Spawn workers equal to CPU cores for parallelism
- **Strict Sharding**: Each worker manages a unique storage shard, eliminating cross-shard lock contention
- **Key-based Affinity**: Commands targeting the same key are routed to the same worker/shard
- **Thread Safety**: All state is isolated or shared via `Arc` with message passing

### 3.2 Worker Architecture

```
┌──────────────────────────────────────────────────────┐
│                        Server                        │
│  ┌───────────────────────────────────────────────┐   │
│  │ Acceptor Loop                                 │   │
│  │ - Listens on TCP port                         │   │
│  │ - Round-robin dispatch to workers             │   │
│  └───────────────────────────────────────────────┘   │
│           │               │               │          │
│           ▼               ▼               ▼          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │
│  │  Worker 0   │  │  Worker 1   │  │  Worker N   │   │
│  │ (Thread 1)  │  │ (Thread 2)  │  │ (Thread N)  │   │
│  └─────────────┘  └─────────────┘  └─────────────┘   │
│           │               │               │          │
│           ▼               ▼               ▼          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │
│  │  Storage 0  │  │  Storage 1  │  │  Storage N  │   │
│  │ (Shard 0)   │  │ (Shard 1)   │  │ (Shard N)   │   │
│  └─────────────┘  └─────────────┘  └─────────────┘   │
└──────────────────────────────────────────────────────┘
```

### 3.3 Worker Lifecycle

#### 3.3.1 Creation (`Worker::new`)

```rust
pub fn new(
    tx: mpsc::UnboundedSender<WorkerMessage>,
    rx: mpsc::UnboundedReceiver<WorkerMessage>,
    peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
    storage: Arc<Storage>,
    cmd_table: Arc<CmdTable>,
) -> Self
```

**Process:**

1. **Spawn Dedicated Thread**: Each worker runs in its own native thread with a dedicated Tokio runtime
2. **Initialize Runtime**: Creates a single-threaded Tokio runtime with `enable_all()`
3. **Store References**: Keeps references to shared resources (storage, cmd_table, peer channels)

#### 3.3.2 Message Loop & Smart Batching

The worker runs an async loop processing `WorkerMessage` variants. It employs **Smart Batching** to drain multiple messages from the channel at once:

```rust
while let Some(msg) = rx.recv().await {
    batch_buffer.push(msg);
    // Drain more if available, up to 256
    while batch_buffer.len() < 256 {
        match rx.try_recv() {
            Ok(msg) => batch_buffer.push(msg),
            Err(_) => break,
        }
    }
    // Process drained batch...
}
```

This reduces the number of async wakeups and improves throughput under high load.

### 3.4 Message Types

#### 3.4.1 NewConnection

When a new client connects, the acceptor dispatches the TCP stream to a worker:

```rust
WorkerMessage::NewConnection(TcpStream)
```

The worker spawns an async task (`tokio::spawn`) to handle the connection independently, allowing concurrent processing of multiple clients.

#### 3.4.2 CmdBatch

Commands are batched by the client handler before being sent to the target worker to minimize inter-thread communication overhead:

```rust
pub enum WorkerMessage {
    NewConnection(TcpStream),
    CmdBatch(Vec<CmdRequest>),
}

pub struct CmdRequest {
    pub(crate) cmd_name: String,
    pub(crate) args: Vec<Bytes>,
    pub(crate) resp_tx: oneshot::Sender<RespValue>,
}
```

### 3.5 Request Routing: Consistent Hashing

#### 3.5.1 Why Consistent Hashing?

To ensure **key-based affinity** - commands targeting the same key must be processed by the same worker. This guarantees:
- **Atomic operations** on the same key are serialized
- **Multi-key operations** see a consistent view across commands

#### 3.5.2 Routing Algorithm: FNV-1a

Nimbis uses the **FNV-1a** hashing algorithm for its speed and good distribution properties on short keys:

```rust
let mut hasher: u64 = 0xcbf29ce484222325;
for byte in &key {
    hasher ^= *byte as u64;
    hasher = hasher.wrapping_mul(0x100000001b3);
}
let target_worker_idx = (hasher as usize) % peers.len();
```

**Steps:**
1. Extract the first argument (typically the key)
2. Compute 64-bit FNV-1a hash
3. Modulo by worker count to get target worker index
4. Route via `CmdBatchDescriptor` to target worker

#### 3.5.3 Channel Communication

```rust
let (resp_tx, resp_rx) = oneshot::channel();

if let Some(sender) = peers.get(&target_worker_idx) {
    sender.send(WorkerMessage::CmdRequest(CmdRequest { ... }))?;
}

let response = resp_rx.await?;
```

**Benefits:**
- **Serial Execution**: All commands for a key go through the same worker's channel
- **Non-blocking**: Async channels don't block the sender
- **Response Channel**: `oneshot` channel ensures response delivery

### 3.6 Client Handling (`handle_client`)

#### 3.6.1 Connection State

Each client connection maintains:
```rust
let mut parser = RespParser::new();
let mut buffer = BytesMut::with_capacity(4096);
```

#### 3.6.2 Read Loop

```rust
loop {
    let n = socket.read_buf(&mut buffer).await?;
    if n == 0 {
        return Ok(());  // Connection closed
    }
    // ... parse buffer ...
}
```

#### 3.6.3 RESP Parsing

The parser supports **pipelining** - multiple commands in a single network packet:

```rust
loop {
    match parser.parse(&mut buffer) {
        RespParseResult::Complete(value) => {
            // Process complete command
        }
        RespParseResult::Incomplete => {
            break;  // Wait for more data
        }
        RespParseResult::Error(e) => {
            // Send error, close connection
        }
    }
}
```

#### 3.6.4 Batching & Ordered Responses

The `handle_client` loop parses multiple RESP values and groups them into batches by target worker. To maintain Redis's serial response guarantee, it uses an `ordered_responses` list:

1. **Parse**: Extract multiple commands from the read buffer.
2. **Route**: For each command, calculate the target worker and push a `oneshot::Receiver` into `ordered_responses`.
3. **Dispatch**: Send `CmdBatch` to each involved worker.
4. **Collect**: Wait for `oneshot` receivers in the exact order they were created and write to the socket.

#### 3.6.5 Multi-Key & Global Commands (Scatter-Gather)

| Command     | Behavior           | implementation                                                      |
| ----------- | ------------------ | ------------------------------------------------------------------- |
| `FLUSHDB`   | **Broadcast**      | Sent to ALL workers; success if all OK.                             |
| `DEL k1 k2` | **Scatter-Gather** | Keys grouped by target worker; sent as sub-batches; results summed. |

### 3.7 Storage Sharding

Each worker owns its own `Storage` instance, rooted under the configured object store URL path:
`{object_store_url path}/shard-{id}/`

This ensures:
- **Zero contention**: No cross-shard locks or shared SlateDB instances.
- **Improved Cache Locality**: Each worker thread processes a specific subset of the data.
- **Independent Compaction**: SlateDB background tasks are distributed across workers.

| Scenario          | Handling                     |
| ----------------- | ---------------------------- |
| `ConnectionReset` | Log debug, close gracefully  |
| Incomplete data   | Wait for more bytes          |
| Malformed RESP    | Send error, close connection |
| Unknown command   | Return `ERR unknown command` |
| Worker dropped    | Log warning, drop response   |

### 3.8 Threading Model

#### 3.8.1 Worker Threads

- **Count**: `num_cpus::get()` (one per CPU core)
- **Type**: Native `std::thread` with Tokio runtime
- **Isolation**: Each worker has its own runtime and event loop

#### 3.8.2 Async Tasks

- **Client handlers**: Spawned via `tokio::spawn` within worker
- **Command execution**: Async I/O to storage layer
- **Inter-worker communication**: `mpsc::unbounded_channel`

#### 3.8.3 Shared State

All mutable state is shared via `Arc`:
- `Arc<Storage>` - Persistent data store
- `Arc<CmdTable>` - Command registry
- `Arc<HashMap<usize, Sender>>` - Worker peer map

**No locks required** - all access is either:
1. Read-only (immutable `Arc` references)
2. Message passing (channels)

### 3.9 Scalability Considerations

#### Current Design
- **Worker count**: Fixed at CPU count
- **Scaling strategy**: Vertical (more cores)

#### Potential Improvements
- **Dynamic worker pool**: Add/remove workers based on load
- **Hot reload**: Update cmd_table without restarting workers
- **Connection pooling**: Share connections between workers

### 3.10 Key Implementation Files

| File                          | Purpose                  |
| ----------------------------- | ------------------------ |
| `nimbis/src/worker.rs` | Worker implementation    |
| `nimbis/src/server.rs` | Server/acceptor loop     |
| `nimbis-resp/src/`            | RESP protocol parser     |
| `nimbis-storage/src/`         | Persistent storage layer |

---

## 4. Error Handling

- **Parsing Errors**: Malformed RESP data results in a protocol error sent to the client, and the connection is closed.
- **Command Errors**: Logical errors (e.g., wrong argument count, type mismatch) return a standard Redis error message (`ERR`, `WRONGTYPE`, etc.) without closing the connection.
- **I/O Errors**: Socket read/write errors are propagated; the handler returns and the connection is closed.
