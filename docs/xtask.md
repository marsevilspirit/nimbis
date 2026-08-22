# Utility Tasks

This document describes the utility tasks exposed by the `xtask` crate.

## Usage

Run tasks through `just` for everyday development:

```bash
just check-workspace
just check-code-fmt
just check-numbered-comments
```

Tasks can also be run directly with Cargo:

```bash
cargo xtask check-workspace
cargo xtask check-code-fmt
cargo xtask check-numbered-comments
```

## Commands

### `check-workspace`

Verifies workspace integrity by checking that:

1. dependencies use `workspace = true` instead of hardcoded version numbers where expected
2. dependencies are sorted alphabetically within each block
3. `Cargo.toml` files do not use tabs

The check scans `Cargo.toml` files in the repository, including the root
manifest, workspace crates at the repository root, and `xtask/Cargo.toml`.
Local path dependencies are skipped.

### `check-code-fmt`

Checks repository-specific Rust formatting conventions that are not covered by
`rustfmt`, including spacing between adjacent `impl` blocks and avoiding
indented `use` statements outside test modules.

### `check-numbered-comments`

Rejects numbered step comments such as `// 1.` in Rust source files. These are
usually development notes and should be removed or rewritten as normal comments.

### `compare-benchmarks`

Compares benchmark outputs and prints a Markdown report.

```bash
cargo xtask compare-benchmarks \
  --main <main_bench_file> \
  --pr <pr_bench_file> \
  --main-pipeline <main_pipeline_file> \
  --pr-pipeline <pr_pipeline_file> \
  --baseline <NAME=PATH> \
  --baseline-pipeline <NAME=PATH>
```

The benchmark workflow uses this command to generate the pull request benchmark
report.

Optional `--main-label`, `--pr-label`, and `--pipeline-depth` arguments customize
the report columns and pipeline heading. Their defaults preserve the CI report's
`Main`, `PR`, and `P=50` labels.

### `redis-benchmark-compare`

Builds two committed Git refs in an isolated temporary clone, runs the Redis
comparison profile against both release binaries, and prints a Markdown report.
Each binary is benchmarked sequentially with fresh local storage at `P=1` and a
configurable pipeline depth (default `P=50`), so the two servers do not compete
for local CPU or I/O resources.

```bash
cargo xtask redis-benchmark-compare --base main --head HEAD
```

The command does not switch the current worktree. `HEAD` refers to its committed
state, so uncommitted changes are not included.

## Requirements

- Rust toolchain as specified in `rust-toolchain.toml`
- `just` for the recommended command entrypoints

No separate `rust-script` installation is required.
