# Scripts Documentation

This document describes the purpose and usage of all scripts in the `scripts/` directory.

## check_workspace_deps.rs

**Purpose**: Verifies workspace integrity by checking that:
1. All dependencies use `workspace = true` instead of hardcoded version numbers
2. Dependencies are sorted alphabetically
3. No tabs are used (spaces preferred)

**Usage**:

```bash
# Run via just command (recommended)
just check-workspace

# Or run directly with rust-script
rust-script scripts/check_workspace_deps.rs
```

**How it works**:
1. Scans `Cargo.toml` in root directory and all files in `crates/` directory
2. Checks `[dependencies]` and `[dev-dependencies]` sections
3. Verifies dependencies use `workspace = true` pattern
4. Checks alphabetical ordering of dependencies
5. Validates formatting (no tabs)
6. Automatically skips local path dependencies (workspace members)

**Requirements**:
- `rust-script` installed: `cargo install rust-script`
- Rust toolchain (as specified in `rust-toolchain.toml`)

**CI Integration Example**:

```yaml
- name: Install rust-script
  run: cargo install rust-script

- name: Check workspace dependencies
  run: just check-workspace
```

**Cross-platform**: ✅ Windows / macOS / Linux
