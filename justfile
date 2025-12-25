set shell := ["bash", "-c"]

# List available recipes
default:
    @just --list

# Build all crates
build:
    cargo build

# Build release
build-release:
    cargo build --release

# Run all tests
test:
    cargo test --workspace

# Check all crates
check:
    cargo check --workspace
    cargo fmt -- --check
    cargo clippy --workspace -- -D warnings

# Format code
fmt:
    cargo fmt --all

# Run all checks (format, clippy, test)
all: fmt check test

# Clean build artifacts
clean:
    cargo clean

# Run nimbis-core
run:
    cargo run -p nimbis-core
