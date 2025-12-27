# Nimbis

A Redis-compatible database built with Rust, using object storage as the backend.


## Roadmap

See [ROADMAP.md](ROADMAP.md) for the detailed development plan and upcoming features.

## Design Philosophy

Nimbis is built on the principle of **never trading off** unless there's a suitable alternative approach.

## Project Structure

- `crates/resp` - RESP protocol implementation
- `crates/telemetry` - Telemetry and logging
- `crates/nimbis` - Nimbis server