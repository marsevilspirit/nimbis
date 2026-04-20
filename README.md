# Nimbis

A Redis-compatible database built with Rust, using object storage as the backend.


## Roadmap

See [ROADMAP.md](ROADMAP.md) for the detailed development plan and upcoming features.

## Features

- **Redis-Compatible Commands**: Comprehensive support for string, hash, list, set, and sorted set data types. See [Commands](docs/commands.md) for the complete list of supported commands and implementation guide.
- **Persistence**: Data is persisted to SlateDB (object storage compatible).
- **Configuration**: Dynamic configuration updates.
- **Observability**: Detailed build and environment information (git hash, branch, rustc version) displayed on startup.

## Design Philosophy

Nimbis is built on the principle of **never trading off** unless there's a suitable alternative approach.

## Project Structure

Nimbis is organized as a Cargo workspace with multiple focused crates:

- `crates/macros` - Procedural macros for derive implementations (e.g., `OnlineConfig`)
- `crates/resp` - RESP protocol parser and encoder
- `crates/storage` - Persistent storage layer using SlateDB
- `crates/telemetry` - Logging and observability
- `crates/nimbis` - Main server executable, command implementations, and configuration management

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

# Install cargo-nextest
cargo install --locked cargo-nextest
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

## Coverage

Rust coverage is collected in GitHub Actions by the workflow in `.github/workflows/coverage.yml`.

- Trigger: push and pull_request on `main`
- Scope: Rust workspace tests only
- Reports: `lcov.info` and `cobertura.xml` (also uploaded as workflow artifacts)

### Codecov Setup

For private repositories, add `CODECOV_TOKEN` in GitHub repository settings:

1. Open repository settings in GitHub.
2. Go to Secrets and variables -> Actions.
3. Create a new repository secret named `CODECOV_TOKEN`.

For public repositories, token usage depends on your Codecov setup.