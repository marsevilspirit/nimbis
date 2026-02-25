# List available recipes
[private]
default:
    @just --list

# Build all crates
[group: 'misc']
build *args:
    cargo build {{args}}

# Run unit tests
[group: 'test']
test:
    cargo nextest run

# Run e2e tests
[group: 'test']
e2e-test: (build "--release")
    rm -rf nimbis_data
    cd tests && go test -timeout 15m --ginkgo.v

# Check all crates
[group: 'check']
check: check-workspace check-code-fmt check-numbered-comments
    cargo check --workspace
    cargo fmt -- --check
    cargo clippy --workspace -- -D warnings
    rustfmt --check scripts/*.rs

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

# Check numbered step comments
[private]
[group: 'check']
check-numbered-comments:
    rust-script scripts/check_numbered_comments.rs

# Format code
[group: 'misc']
fmt:
    cargo fmt --all
    rustfmt scripts/*.rs

# Clean build artifacts
[group: 'clean']
clean:
    cargo clean
    rm -rf nimbis_data

# Run nimbis-server
[group: 'misc']
run *args:
    cargo run -p nimbis {{args}}
