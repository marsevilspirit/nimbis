# Nimbis Server Design & process

This document describes the design principles, architecture, and running process of the Nimbis server.

## 1. Architecture Overview

Nimbis is designed as an asynchronous, multi-threaded Redis-compatible server built using Rust and the Tokio runtime.

### 1.1 Concurrency Model
The server follows a classic "Acceptor-Worker" pattern using Tokio's lightweight tasks:
- **Main Task (Acceptor)**: Responsible for binding to the TCP port and accepting incoming connections. It spawns a new task for each client.
- **Client Tasks (Workers)**: Each connected client is handled by its own independent Tokio task. This ensures that one slow client does not block others.

### 1.2 Data Storage
- **Persistent Storage**: Data is stored persistently using `SlateDB` via the `Storage` struct.
- **Interface**: The `Storage` struct provides asynchronous `get` and `set` methods.
- **Thread Safety**: The storage handle is wrapped in `Arc` (`Arc<Storage>`) for safe concurrent access.

### 1.3 Protocol
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
    addr: String,
    storage: Arc<Storage>, // Concrete storage handle
}
```

### 2.2 Lifecycle

#### Step 1: Initialization (`new`)
When `Server::new(addr)` is called:
1. It initializes the `addr` field.
2. It opens the persistent storage (`Storage::open("./nimbis_data")`).

#### Step 2: Running (`run`)
The `run` method is the entry point for the server's execution loop:
1. **Bind**: It creates a `TcpListener` bound to the configured address.
2. **Accept Loop**: It enters an infinite loop waiting for connections.
   - On `listener.accept()` success:
     - It clones the `storage` handle (cheap pointer clone).
     - It spawns a `tokio::spawn` task to run `handle_client`.

---

## 3. Request Processing Flow

The `handle_client` function manages the interaction with a connected client.

### 3.1 Connection State
- **Socket**: The `TcpStream` for reading/writing.
- **Buffer**: A `BytesMut` buffer is allocated (default 4KB) to accumulate incoming data.

### 3.2 processing Loop
The client handler runs in a loop to process pipelined requests:

1. **Read Network Data**:
   - Calls `socket.read_buf(&mut buffer)`.
   - If 0 bytes are read, the connection is closed.

2. **Parse & Execute Loop**:
   - The buffer is strictly processed in a loop to handle multiple commands in a single packet (pipelining).
   - **Parse**: Calls `resp::parse(&mut buffer)` from the `nimbis-resp` crate.
     - This function attempts to decode a complete RESP frame.
     - It advances the buffer cursor automatically.

3. **Command Execution**:
   - **Convert**: The raw `RespValue` is converted into a `ParsedCmd` struct.
     - Validates the structure (must be an Array).
     - extracts the command name (normalized to UPPERCASE).
   - **Lookup**: The command name is looked up in the `CmdTable` (from the `command` crate).
   - **Run**:
     - **Found**: Calls `cmd.execute(&storage, &args)`.
     - **Not Found**: Returns an "unknown command" error.


4. **Response**:
   - The result (`RespValue`) is encoded back into bytes.
   - Sent to the client via `socket.write_all`.

### 3.3 Error Handling
- **Parsing Errors**: Malformed RESP data results in a protocol error sent to the client, and the connection is usually closed or reset.
- **Command Errors**: Logical errors (e.g., wrong arg count) return a standard Redis `ERR` or `WRONGTYPE` message without closing the connection.
