# List available recipes
default:
    @just --list

# Build all crates
build:
    cargo build

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

# Clean build artifacts
clean:
    cargo clean

# Run nimbis-server
run:
    cargo run -p nimbis-server
