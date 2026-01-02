# Nimbis

A Redis-compatible database built with Rust, using object storage as the backend.


## Roadmap

See [ROADMAP.md](ROADMAP.md) for the detailed development plan and upcoming features.

## Features

- **Core Commands**: `PING`, `GET`, `SET`, `CONFIG GET`, `CONFIG SET`
- **Persistence**: Data is persisted to SlateDB (object storage compatible).
- **Configuration**: Dynamic configuration updates via `CONFIG SET`.

## Design Philosophy

Nimbis is built on the principle of **never trading off** unless there's a suitable alternative approach.

## Project Structure

Nimbis is organized as a Cargo workspace with multiple focused crates:

- `crates/nimbis` - Main server executable and command implementations
- `crates/config` - Configuration management with derive macros
- `crates/resp` - RESP protocol parser and encoder
- `crates/storage` - Persistent storage layer using SlateDB
- `crates/telemetry` - Logging and observability

For detailed information about the crate organization, see [Crates Organization](docs/crates_organization.md).

## Development

### Prerequisites

- Rust (latest stable)
- Go (for integration tests)
- `just` command runner

### Common Commands

```bash
# Build the project
just build

# Run the server
just run

# Run unit tests
just test

# Run End-to-End integration tests
just e2e-test

# Check code quality (format, clippy)
just check
```