# List available recipes
[private]
default:
    @just --list

# Build all crates
build:
    cargo build

# Run unit tests
test:
    cargo nextest run

# Run e2e tests
e2e-test:
    cd tests && go test -timeout 15m --ginkgo.v
    
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
