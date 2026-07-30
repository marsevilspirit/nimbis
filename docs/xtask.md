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
  --main <main_round_1> \
  --pr <pr_round_1> \
  --main <main_round_2> \
  --pr <pr_round_2> \
  --main-pipeline <main_pipeline_round_1> \
  --pr-pipeline <pr_pipeline_round_1> \
  --main-pipeline <main_pipeline_round_2> \
  --pr-pipeline <pr_pipeline_round_2> \
  --baseline <NAME=PATH> \
  --baseline-pipeline <NAME=PATH>
```

`--main` and `--pr` can be repeated and must be supplied in paired order; the
same applies to their pipeline variants. The report uses median RPS, the median
of per-pair percentage changes, MAD, and the observed min/max range. The
benchmark workflow uses this command to generate the pull request report.
Repeated baseline inputs with the same name are grouped into an unpaired median
and labeled as informational.

## Requirements

- Rust toolchain as specified in `rust-toolchain.toml`
- `just` for the recommended command entrypoints

No separate `rust-script` installation is required.
