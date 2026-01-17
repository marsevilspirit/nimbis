# Worker Design & Implementation

This document describes the design and implementation of Nimbis's Worker component.

## 1. Overview

The Worker subsystem is responsible for handling client commands and managing connections. It implements a multi-worker architecture with consistent hashing for request routing.

### 1.1 Key Design Goals

- **Multi-core Utilization**: Spawn workers equal to CPU cores for parallelism
- **Strict Sharding**: Each worker manages a unique storage shard, eliminating cross-shard lock contention
- **Key-based Affinity**: Commands targeting the same key are routed to the same worker/shard
- **Thread Safety**: All state is isolated or shared via `Arc` with message passing

---

## 2. Architecture

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

### 3.2 Message Loop & Smart Batching

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

---

## 4. Message Types

### 4.1 NewConnection

When a new client connects, the acceptor dispatches the TCP stream to a worker:

```rust
WorkerMessage::NewConnection(TcpStream)
```

The worker spawns an async task (`tokio::spawn`) to handle the connection independently, allowing concurrent processing of multiple clients.

### 4.2 CmdBatch

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

---

## 5. Request Routing: Consistent Hashing

### 5.1 Why Consistent Hashing?

To ensure **key-based affinity** - commands targeting the same key must be processed by the same worker. This guarantees:
- **Atomic operations** on the same key are serialized
- **Multi-key operations** see a consistent view across commands

### 5.2 Routing Algorithm: FNV-1a

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

### 6.4 Batching & Ordered Responses

The `handle_client` loop parses multiple RESP values and groups them into batches by target worker. To maintain Redis's serial response guarantee, it uses an `ordered_responses` list:

1. **Parse**: Extract multiple commands from the read buffer.
2. **Route**: For each command, calculate the target worker and push a `oneshot::Receiver` into `ordered_responses`.
3. **Dispatch**: Send `CmdBatch` to each involved worker.
4. **Collect**: Wait for `oneshot` receivers in the exact order they were created and write to the socket.

### 6.5 Multi-Key & Global Commands (Scatter-Gather)

| Command     | Behavior           | implementation                                                      |
| ----------- | ------------------ | ------------------------------------------------------------------- |
| `FLUSHDB`   | **Broadcast**      | Sent to ALL workers; success if all OK.                             |
| `DEL k1 k2` | **Scatter-Gather** | Keys grouped by target worker; sent as sub-batches; results summed. |

---

## 7. Storage Sharding

Each worker owns its own `Storage` instance, which maps to a unique subdirectory:
`{data_path}/shard-{id}/`

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

| File                          | Purpose                  |
| ----------------------------- | ------------------------ |
| `crates/nimbis/src/worker.rs` | Worker implementation    |
| `crates/nimbis/src/server.rs` | Server/acceptor loop     |
| `crates/resp/src/`            | RESP protocol parser     |
| `crates/storage/src/`         | Persistent storage layer |
