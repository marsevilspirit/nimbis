# List available recipes
[private]
default:
    @just --list

# Build all crates
[group: 'misc']
build *args:
    cargo build {{args}}

# Run unit tests with coverage generation
[group: 'test']
test:
    cargo llvm-cov nextest --all --codecov --output-path codecov.json

# Run e2e tests
[group: 'test']
e2e-test: (build "--release")
    rm -rf nimbis_store
    cd e2e-test && go test -timeout 15m --ginkgo.v

# Run benchmarks for all crates, or for a specific package when PACKAGE is provided
[group: 'test']
bench package="" *args:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -z "{{package}}" ]; then
        cargo bench --workspace {{args}}
    else
        cargo bench -p {{package}} {{args}}
    fi

# Check all crates
[group: 'check']
check: check-workspace check-code-fmt check-numbered-comments
    cargo check --workspace
    cargo fmt -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Check workspace dependencies
[private]
[group: 'check']
check-workspace:
    cargo xtask check-workspace

# Check code format
[private]
[group: 'check']
check-code-fmt:
    cargo xtask check-code-fmt

# Check numbered step comments
[private]
[group: 'check']
check-numbered-comments:
    cargo xtask check-numbered-comments

# Format code
[group: 'misc']
fmt:
    cargo fmt --all

# Clean build artifacts
[group: 'clean']
clean:
    cargo clean
    rm -rf nimbis_store

# Run nimbis-server
[group: 'misc']
run *args:
    cargo run -p nimbis {{args}}
