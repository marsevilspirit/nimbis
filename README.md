# Nimbis

A Redis-compatible database built with Rust, using object storage as the backend.


## Roadmap

See [ROADMAP.md](ROADMAP.md) for the detailed development plan and upcoming features.

## Features

- **Redis-Compatible Commands**: Comprehensive support for string, hash, list, set, and zset types. See [Command Arity](docs/command_arity.md) for the complete list of supported commands.
- **Persistence**: Data is persisted to SlateDB (object storage compatible).
- **Configuration**: Dynamic configuration updates.
- **Observability**: Detailed build and environment information (git hash, branch, rustc version) displayed on startup.

## Design Philosophy

Nimbis is built on the principle of **never trading off** unless there's a suitable alternative approach.

## Project Structure

Nimbis is organized as a Cargo workspace with multiple focused crates:

- `crates/nimbis` - Main server executable, command implementations, and configuration management
- `crates/resp` - RESP protocol parser and encoder
- `crates/storage` - Persistent storage layer using SlateDB
- `crates/telemetry` - Logging and observability

For detailed information about the crate organization, see [Crates Organization](docs/crates_organization.md).

## Development

### Prerequisites

- **Rust**: Latest stable version
- **Go**: Required for integration tests
- **Just**: Command runner
- **rust-script**: Required for running utility scripts

**Install dependencies:**

```bash
# Install rust-script
cargo install rust-script

# Install just
cargo install just
```

### Common Commands

```
$ just
Available recipes:
    [check]
    check       # Check all crates

    [clean]
    clean       # Clean build artifacts

    [misc]
    build *args # Build all crates
    fmt         # Format code
    run *args   # Run nimbis-server

    [test]
    e2e-test    # Run e2e tests
    test        # Run unit tests
```