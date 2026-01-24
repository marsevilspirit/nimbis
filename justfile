# List available recipes
[private]
default:
    @just --list

# Build all crates
[group: 'misc']
build profile='release':
    cargo build {{ if profile == 'release' { "--release" } else { "" } }}

# Run unit tests
[group: 'test']
test:
    cargo nextest run

# Run e2e tests
[group: 'test']
e2e-test: build
    rm -rf nimbis_data
    cd tests && go test -timeout 15m --ginkgo.v

# Check all crates
[group: 'check']
check: check-workspace check-code-fmt
    cargo check --workspace
    cargo fmt -- --check
    cargo clippy --workspace -- -D warnings

# Check workspace dependencies
[private]
[group: 'check']
check-workspace:
    rust-script scripts/check_workspace_deps.rs

# Check code format
[private]
[group: 'check']
check-code-fmt:
    rust-script scripts/check_code_fmt.rs

# Format code
[group: 'misc']
fmt:
    cargo fmt --all

# Clean build artifacts
[group: 'clean']
clean:
    cargo clean
    rm -rf nimbis_data

# Run nimbis-server
[group: 'misc']
run:
    cargo run -p nimbis
