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
	rm -rf nimbis_data
	cd tests && go test -timeout 15m --ginkgo.v
    
# Check all crates
check:
    just check-workspace
    cargo check --workspace
    cargo fmt -- --check
    cargo clippy --workspace -- -D warnings

# Check workspace dependencies
[private]
check-workspace:
    rust-script scripts/check_workspace_deps.rs

# Format code
fmt:
    cargo fmt --all

# Clean build artifacts
clean:
    cargo clean
    rm -rf nimbis_data

# Run nimbis-server
run:
    cargo run -p nimbis
