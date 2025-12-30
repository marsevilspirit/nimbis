# Nimbis

A Redis-compatible database built with Rust, using object storage as the backend.


## Roadmap

See [ROADMAP.md](ROADMAP.md) for the detailed development plan and upcoming features.

## Design Philosophy

Nimbis is built on the principle of **never trading off** unless there's a suitable alternative approach.

## Project Structure

Nimbis is organized as a Cargo workspace with multiple focused crates:

- `crates/command` - Command system framework and built-in commands
- `crates/config` - Configuration management with derive macros
- `crates/resp` - RESP protocol parser and encoder
- `crates/storage` - Persistent storage layer using SlateDB
- `crates/telemetry` - Logging and observability
- `crates/nimbis` - Main server executable

For detailed information about the crate organization, see [Crates Organization](docs/crates_organization.md).