# Scripts Documentation

This document describes the purpose and usage of all scripts in the `scripts/` directory.

## check_workspace_deps.rs

**Purpose**: Verifies that all crate dependencies use `workspace = true` instead of hardcoded version numbers.

**Usage**:

```bash
# Run via just command (recommended)
just check-workspace

# Or run directly
rust-script scripts/check_workspace_deps.rs
```

**How it works**:
1. Scans all `Cargo.toml` files in the `crates/` directory
2. Checks `[dependencies]` and `[dev-dependencies]` sections
3. Reports any dependencies not using `workspace = true`
4. Automatically skips local path dependencies (workspace members)

**Requirements**:
- Requires `rust-script` to be installed: `cargo install rust-script`

**CI Integration Example**:

```yaml
- name: Install rust-script
  run: cargo install rust-script

- name: Check workspace dependencies
  run: just check-workspace
```

**Cross-platform**: ✅ Windows / macOS / Linux
