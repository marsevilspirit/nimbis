# Project Overview

Nimbis is a Redis-compatible database written in Rust, with object storage as
the persistence backend.

The repository is organized as a Cargo workspace with focused crates for the
server, RESP protocol handling, storage, telemetry, macros, and workspace
tooling.

# Tech Stack

- Rust 2024 edition for the main workspace.
- Go for end-to-end integration tests.
- Cargo workspace for crate organization and dependency sharing.
- Just for common development commands.
- SlateDB and object-store compatible backends for persistence.
- Tokio-based async runtime and structured logging/telemetry support.

# Build/Test Commands

- `just build` builds all crates.
- `just check` runs workspace checks, formatting checks, `cargo check`, and
  Clippy with warnings denied.
- `just fmt` formats Rust code.
- `just test` runs Rust unit tests with coverage output.
- `just e2e-test` runs Go end-to-end tests.
- `just bench` runs storage benchmarks.
- `just run` runs the Nimbis server.

# Coding Conventions

- Follow the existing workspace crate boundaries.
- Keep changes scoped to the relevant crate or command path.
- Use `cargo fmt`/`just fmt` formatting.
- Keep Clippy clean under the repository's configured checks.
- Prefer typed errors and explicit result handling over panics in production
  paths.
- Use the configured logging facade instead of direct stdout/stderr output.

# Testing Conventions

- Prefer `rstest` for Rust tests.
- Add unit tests near the code they exercise when behavior is local.
- Add integration or e2e coverage when behavior crosses crate, protocol, or
  server boundaries.
- Use existing Go/Ginkgo e2e patterns for Redis-compatible command behavior.
- Keep benchmark changes focused on performance-sensitive storage paths.

# PR Conventions

- Keep PRs focused on one logical change.
- PR titles must use one of these prefixes: `feat:`, `fix:`, `refactor:`, or
  `test:`.
- PR descriptions must be written in English.
- Include the user-visible behavior change, notable implementation notes, and
  validation commands.

# Constraints

- Preserve Redis protocol compatibility unless a change explicitly updates the
  supported behavior.
- Preserve object-storage persistence semantics.
- Avoid broad refactors that are unrelated to the requested change.
- Keep configuration defaults and documented paths compatible.
- Treat storage, protocol parsing, and telemetry lifecycle changes as
  high-care areas.

# Definition of Done

- The change is scoped, readable, and follows existing repository patterns.
- Relevant tests or checks have been added or updated.
- `just check` passes, or any skipped validation is clearly explained.
- User-facing documentation is updated when behavior or commands change.
- PR title and description follow the repository conventions.
