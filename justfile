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
e2e-test:
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

# Run redis-benchmark against a running Nimbis server
[group: 'test']
redis-bench *args:
    cargo xtask redis-benchmark {{args}}

# Run all explicit-count list-pop cells with paired P=1/P=50 passes
[positional-arguments]
[group: 'test']
redis-bench-counted-pop base_binary="target/release/nimbis" head_binary="target/release/nimbis" output_dir="target/counted-pop" *args:
    #!/usr/bin/env bash
    set -euo pipefail
    counted_base="$1"
    counted_head="$2"
    counted_output="$3"
    shift 3
    counted_commands=lpop-count-1,lpop-count-2,lpop-count-32,lpop-count-256,rpop-count-1,rpop-count-2,rpop-count-32,rpop-count-256
    cargo xtask benchmark-ci-shard \
      --main-binary "$counted_base" --pr-binary "$counted_head" \
      --commands "$counted_commands" --data-size "${D:-128}" \
      --requests "${N:-1000}" --clients "${C:-20}" --replica 1 \
      --output-dir "$counted_output" "$@"
    cargo xtask benchmark-ci-report --input-dir "$counted_output" \
      --output "$counted_output/report.md" --expected-replicas 1 \
      --expected-data-sizes "${D:-128}" --expected-commands "$counted_commands"

# Compare redis-benchmark results for two Git refs
[positional-arguments]
[group: 'test']
redis-bench-compare *args:
    #!/usr/bin/env bash
    ref_args=()
    if [ "$#" -gt 0 ] && [[ "$1" != -* ]]; then
        ref_args+=(--base "$1")
        shift
    fi
    if [ "$#" -gt 0 ] && [[ "$1" != -* ]]; then
        ref_args+=(--head "$1")
        shift
    fi
    exec cargo xtask redis-benchmark-compare "${ref_args[@]}" "$@"

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
