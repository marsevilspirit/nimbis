# Nimbis

A Redis-compatible database built with Rust, using object storage as the backend.

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/marsevilspirit/nimbis)


## Roadmap

See [ROADMAP.md](ROADMAP.md) for the detailed development plan and upcoming features.

## Features

- **Redis-Compatible Commands**: Comprehensive support for string, hash, list, set, and sorted set data types. See [Commands](docs/commands.md) for the complete list of supported commands and implementation guide.
- **Persistence**: Data is persisted to [SlateDB](https://github.com/slatedb/slatedb) (object storage compatible).
- **Configuration**: Dynamic configuration updates.
- **Observability**: Detailed build and environment information (git hash, branch, rustc version) displayed on startup.

## Design Philosophy

Nimbis is built on the principle of **never trading off** unless there's a suitable alternative approach.

## Project Structure

Nimbis is organized as a Cargo workspace with multiple focused crates:

- `nimbis-macros` - Procedural macros for derive implementations (e.g., `OnlineConfig`)
- `nimbis-resp` - RESP protocol parser and encoder
- `nimbis-storage` - Persistent storage layer using SlateDB
- `nimbis-telemetry` - Logging and observability
- `nimbis` - Main server executable, command implementations, and configuration management

For detailed information about the crate organization, see [Crates Organization](docs/crates_organization.md).

## Development

### Prerequisites

- **Rust**: Latest stable version
- **Go**: Required for integration tests
- **Just**: Command runner

**Install dependencies:**

```bash
# Install just
cargo install just

# Install cargo-nextest
cargo install --locked cargo-nextest

# Install cargo-llvm-cov
cargo install cargo-llvm-cov
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
    bench       # Run storage benchmark target
    e2e-test    # Run e2e tests
    test        # Run unit tests
```

Default configuration path is `config/config.toml`. Legacy `conf/config.toml` is still supported as a fallback.
