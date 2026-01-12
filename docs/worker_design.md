# Worker Design & Implementation

This document describes the design and implementation of Nimbis's Worker component.

## 1. Overview

The Worker subsystem is responsible for handling client commands and managing connections. It implements a multi-worker architecture with consistent hashing for request routing.

### 1.1 Key Design Goals

- **Multi-core Utilization**: Spawn workers equal to CPU cores for parallelism
- **Connection Isolation**: Each worker handles connections independently
- **Key-based Affinity**: Commands targeting the same key are routed to the same worker
- **Thread Safety**: All state is shared via `Arc` with message passing

---

## 2. Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Server                               │
│  ┌─────────────────────────────────────────────────────┐    │
│  │ Acceptor Loop                                       │    │
│  │ - Listens on TCP port                               │    │
│  │ - Round-robin dispatch to workers                   │    │
│  └─────────────────────────────────────────────────────┘    │
│           │               │               │                 │
│           ▼               ▼               ▼                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐          │
│  │  Worker 0   │  │  Worker 1   │  │  Worker N   │          │
│  │ (Thread 1)  │  │ (Thread 2)  │  │ (Thread N)  │          │
│  └─────────────┘  └─────────────┘  └─────────────┘          │
│           │               │               │                 │
│           └───────────────┴───────────────┘                 │
│                         │                                   │
│              ┌──────────┴──────────┐                        │
│              │    Consistent       │                        │
│              │      Hashing        │                        │
│              │  (Key → Worker ID)  │                        │
│              └─────────────────────┘                        │
│                         │                                   │
│              ┌──────────┴──────────┐                        │
│              │       Storage       │                        │
│              │     (Arc<Storage>)  │                        │
│              └─────────────────────┘                        │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Worker Lifecycle

### 3.1 Creation (`Worker::new`)

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

### 3.2 Message Loop

The worker runs an async loop processing `WorkerMessage` variants:

```rust
while let Some(msg) = rx.recv().await {
    match msg {
        WorkerMessage::NewConnection(socket) => {
            // Spawn async task to handle client
        }
        WorkerMessage::CmdRequest(req) => {
            // Execute command and send response
        }
    }
}
```

---

## 4. Message Types

### 4.1 NewConnection

When a new client connects, the acceptor dispatches the TCP stream to a worker:

```rust
WorkerMessage::NewConnection(TcpStream)
```

The worker spawns an async task (`tokio::spawn`) to handle the connection independently, allowing concurrent processing of multiple clients.

### 4.2 CmdRequest

Commands are wrapped in a `CmdRequest` for execution:

```rust
pub struct CmdRequest {
    pub(crate) cmd_name: String,           // Command name (e.g., "GET", "SET")
    pub(crate) args: Vec<Bytes>,           // Command arguments
    pub(crate) resp_tx: oneshot::Sender<RespValue>,  // Response channel
}
```

---

## 5. Request Routing: Consistent Hashing

### 5.1 Why Consistent Hashing?

To ensure **key-based affinity** - commands targeting the same key must be processed by the same worker. This guarantees:
- **Atomic operations** on the same key are serialized
- **Multi-key operations** see a consistent view across commands

### 5.2 Routing Algorithm

```rust
// Calculate target worker using hash of the first key
let hash_key = parsed_cmd.args.first().cloned().unwrap_or_default();
let mut hasher = DefaultHasher::new();
hash_key.hash(&mut hasher);
let target_worker_idx = (hasher.finish() as usize) % peers.len();
```

**Steps:**
1. Extract the first argument (typically the key)
2. Compute 64-bit hash using `DefaultHasher`
3. Modulo by worker count to get target worker index
4. Route via channel to target worker

### 5.3 Channel Communication

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

---

## 6. Client Handling (`handle_client`)

### 6.1 Connection State

Each client connection maintains:
```rust
let mut parser = RespParser::new();
let mut buffer = BytesMut::with_capacity(4096);
```

### 6.2 Read Loop

```rust
loop {
    let n = socket.read_buf(&mut buffer).await?;
    if n == 0 {
        return Ok(());  // Connection closed
    }
    // ... parse buffer ...
}
```

### 6.3 RESP Parsing

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

### 6.4 Command Execution Flow

```
┌──────────────┐
│ Read bytes   │
└──────┬───────┘
       ▼
┌──────────────┐
│ Parse RESP   │
└──────┬───────┘
       ▼
┌──────────────┐
│ Extract key, │
│ compute hash │
└──────┬───────┘
       ▼
┌──────────────┐
│ Route to     │◄─────────────────────┐
│ target worker│                      │
└──────┬───────┘                      │
       ▼                              │
┌──────────────┐                      │
│ Execute cmd  │                      │
└──────┬───────┘                      │
       ▼                              │
┌──────────────┐                      │
│ Send response│                      │
└──────┬───────┘                      │
       ▼                              │
Write to socket ──────────────────────┘
```

---

## 7. Error Handling

| Scenario | Handling |
|----------|----------|
| `ConnectionReset` | Log debug, close gracefully |
| Incomplete data | Wait for more bytes |
| Malformed RESP | Send error, close connection |
| Unknown command | Return `ERR unknown command` |
| Worker dropped | Log warning, drop response |

---

## 8. Threading Model

### 8.1 Worker Threads

- **Count**: `num_cpus::get()` (one per CPU core)
- **Type**: Native `std::thread` with Tokio runtime
- **Isolation**: Each worker has its own runtime and event loop

### 8.2 Async Tasks

- **Client handlers**: Spawned via `tokio::spawn` within worker
- **Command execution**: Async I/O to storage layer
- **Inter-worker communication**: `mpsc::unbounded_channel`

### 8.3 Shared State

All mutable state is shared via `Arc`:
- `Arc<Storage>` - Persistent data store
- `Arc<CmdTable>` - Command registry
- `Arc<HashMap<usize, Sender>>` - Worker peer map

**No locks required** - all access is either:
1. Read-only (immutable `Arc` references)
2. Message passing (channels)

---

## 9. Scalability Considerations

### Current Design
- **Worker count**: Fixed at CPU count
- **Scaling strategy**: Vertical (more cores)

### Potential Improvements
- **Dynamic worker pool**: Add/remove workers based on load
- **Hot reload**: Update cmd_table without restarting workers
- **Connection pooling**: Share connections between workers

---

## 10. Key Files

| File | Purpose |
|------|---------|
| `crates/nimbis/src/worker.rs` | Worker implementation |
| `crates/nimbis/src/server.rs` | Server/acceptor loop |
| `crates/resp/src/` | RESP protocol parser |
| `crates/storage/src/` | Persistent storage layer |
