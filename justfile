# List available recipes
[private]
default:
    @just --list

# Build all crates
build:
    cargo build

# Run all tests
test:
    cargo nextest run

# Run integration tests (Go)
test-int:
    cd tests && go test -v .

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
    cargo run -p nimbis
