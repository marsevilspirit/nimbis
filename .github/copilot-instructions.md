# Copilot Cloud Agent Instructions for `nimbis`

## Project snapshot
- Rust workspace (edition 2024) implementing a Redis-compatible database backed by SlateDB/object storage.
- Main crates:
  - `crates/nimbis`: server, command dispatch, runtime config
  - `crates/storage`: typed storage layer over SlateDB
  - `crates/resp`: RESP parser/encoder
  - `crates/macros`: proc macros (notably `OnlineConfig`)
  - `crates/telemetry`: logging/tracing setup
- Integration tests are Go-based in `e2e-test/` and run against a real `nimbis` process.

## Toolchain and prerequisites
- Rust toolchain is pinned to `nightly` (`rust-toolchain.toml`).
- Required CLIs used by repo tasks:
  - `just`
  - `cargo-nextest`
  - Go toolchain (for `e2e-test/`)

Install quickly (if missing):

```bash
cargo install just
cargo install --locked cargo-nextest
```

## Canonical commands (run from repo root)
- Full checks (same shape as CI):
  - `just check`
  - `just build --release`
  - `just test`
  - `just e2e-test`
- Other common commands:
  - `just run`
  - `just fmt`

## Validation expectations
- Prefer `just` recipes over ad-hoc commands.
- Before and after non-trivial code changes, follow this order:
  1. `just check`
  2. `just build --release`
  3. `just test`
  4. `just e2e-test`
- `just e2e-test` removes `nimbis_store`, starts the `nimbis` process, and runs Go/Ginkgo tests that connect to `localhost:6379`.

## Repository-specific guardrails
- Keep `Cargo.toml` dependency entries sorted and prefer `workspace = true` where expected (`cargo xtask check-workspace` enforces this).
- Keep formatting and structural conventions clean (`cargo xtask check-code-fmt`).
- Do not add numbered step comments in code (`cargo xtask check-numbered-comments`).
- For command implementation work, update all three places:
  1. `crates/nimbis/src/cmd/cmd_*.rs`
  2. `crates/nimbis/src/cmd/mod.rs`
  3. `crates/nimbis/src/cmd/table.rs`

## Where to read first for common tasks
- High-level overview: `README.md`
- Command system and supported commands: `docs/commands.md`
- Crate layout: `docs/crates_organization.md`
- Runtime config behavior: `docs/config.md`
- Storage error model: `docs/error_handling.md`
- Go integration testing model: `docs/go_integration_tests.md`

## Errors encountered during onboarding and workarounds
1. **Error:** `just: command not found` when running validation.
   - **Workaround:** install required tools first:
     - `cargo install just`
     - `cargo install --locked cargo-nextest`

2. **Error:** installing `cargo-nextest` without lockfile failed with:
   - `Nextest does not support being installed without --locked`
   - **Workaround:** use:
     - `cargo install --locked cargo-nextest`
