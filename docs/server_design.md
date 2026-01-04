# Nimbis Server Design & process

This document describes the design principles, architecture, and running process of the Nimbis server.

## 1. Architecture Overview

Nimbis is designed as an asynchronous, multi-threaded Redis-compatible server built using Rust and the Tokio runtime.

### 1.1 Concurrency Model
The server follows a classic "Acceptor-Worker" pattern using Tokio's lightweight tasks:
- **Main Task (Acceptor)**: Responsible for binding to the TCP port and accepting incoming connections. It spawns a new task for each client.
- **Client Tasks (Workers)**: Each connected client is handled by its own independent Tokio task. This ensures that one slow client does not block others.

### 1.2 Data Storage
- **Persistent Storage**: Data is stored persistently using multiple `SlateDB` engines via the `Storage` struct.
- **Interface**: The `Storage` struct provides asynchronous methods like `get`, `set`, `hset`, etc.
- **Thread Safety**: The storage handle is wrapped in `Arc` (`Arc<Storage>`) for safe concurrent access across worker tasks.

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
The `Server` struct holds the state required to run the application:
```rust
pub struct Server {
    storage: Arc<Storage>,      // Concrete storage handle
    cmd_table: Arc<CmdTable>,   // Command dispatch table
}
```

### 2.2 Lifecycle

#### Step 1: Configuration Initialization
Before creating the server, the main application initializes the configuration:
```rust
config::init_config();       // Load configuration (addr, data_path)
telemetry::init();           // Initialize logging/tracing
```

The configuration is stored in a thread-safe global state (`SERVER_CONF`) using `OnceLock` and `ArcSwap` for lock-free concurrent access.

#### Step 2: Server Creation (`new`)
When `Server::new()` is called:
1. Reads configuration from `SERVER_CONF.load()`.
2. Creates the data directory if it doesn't exist.
3. Opens persistent storage via `Storage::open(data_path)`.
4. Creates a new `CmdTable` with all registered commands.
5. Wraps both in `Arc` for thread-safe sharing.

#### Step 3: Server Execution (`run`)
The `run` method is the entry point for the server's execution loop:
1. **Bind**: It creates a `TcpListener` bound to the configured address from `SERVER_CONF`.
2. **Accept Loop**: It enters an infinite loop waiting for connections.
   - On `listener.accept()` success:
     - It clones the `storage` and `cmd_table` handles (cheap pointer clones).
     - It spawns a `tokio::spawn` task to run `handle_client`.
   - On error: It logs the error and continues listening.

---

## 3. Request Processing Flow

The `handle_client` function manages the interaction with a connected client and is invoked for each client connection.

### 3.1 Function Signature
```rust
async fn handle_client(
    mut socket: TcpStream,
    storage: Arc<Storage>,
    cmd_table: Arc<CmdTable>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
```

Each client handler receives:
- `socket`: The TCP connection for bidirectional communication
- `storage`: A shared reference to the persistent storage
- `cmd_table`: A shared reference to the command dispatch table

### 3.2 Connection State
- **Socket**: The `TcpStream` for reading/writing.
- **Parser**: A `RespParser` instance for parsing RESP protocol data.
- **Buffer**: A `BytesMut` buffer (4096 bytes by default) to accumulate incoming data.

### 3.3 Processing Loop
The client handler runs in an outer loop to handle multiple network reads (pipelining and fragmented data):

1. **Read Network Data**:
   - Calls `socket.read_buf(&mut buffer)` to read from the socket.
   - If 0 bytes are read, the connection is closed normally.
   - Handles `ConnectionReset` errors gracefully (client crash or abrupt shutdown).

2. **Parse Loop**:
   - Runs an inner loop to process all complete commands in the buffer (supports pipelining).
   - Calls `parser.parse(&mut buffer)` using `RespParser` to extract one RESP frame.
   - Returns one of three states:

     - **`RespParseResult::Complete(value)`**: A complete RESP message is available.
       - Converts the `RespValue` to a `ParsedCmd` struct (validates structure, extracts command name).
       - Proceeds to command execution (see step 3 below).

     - **`RespParseResult::Incomplete`**: The buffer doesn't contain a complete message.
       - Breaks the parse loop to wait for more socket data.

     - **`RespParseResult::Error(e)`**: Malformed RESP data.
       - Sends a protocol error response to the client.
       - Closes the connection.

3. **Command Execution**:
   - **Convert**: The raw `RespValue` is converted into a `ParsedCmd` struct.
     - Validates the structure (must be a non-empty Array).
     - Extracts the command name and normalizes to UPPERCASE.
     - Captures all remaining array elements as command arguments.

   - **Lookup**: The command name is looked up in the `CmdTable`.

   - **Execute**:
     - **Found**: Calls `cmd.execute(&storage, &parsed_cmd.args).await`.
       - Commands are async and may perform I/O operations (storage access).
       - Returns a `RespValue` response.
     - **Not Found**: Returns an "unknown command" error response (e.g., `ERR unknown command 'foo'`).

4. **Response**:
   - The result `RespValue` is encoded into RESP bytes.
   - Sent to the client via `socket.write_all(&encoded)`.
   - Handles `ConnectionReset` errors gracefully.

### 3.4 Error Handling
- **Parsing Errors**: Malformed RESP data results in a protocol error sent to the client, and the connection is closed.
- **Command Errors**: Logical errors (e.g., wrong argument count, type mismatch) return a standard Redis error message (`ERR`, `WRONGTYPE`, etc.) without closing the connection.
- **I/O Errors**: Socket read/write errors are propagated; the handler returns and the connection is closed.
