# Roadmap

Nimbis is an early-stage Redis-compatible database backed by object storage.
The near-term roadmap prioritizes a correct, documented Redis subset before
expanding command breadth.

## Current Baseline

- Server command registration lives in `nimbis/src/cmd/table.rs`.
- The implemented command list is documented in `docs/commands.md`.
- Redis benchmark coverage is documented in `docs/redis-benchmark.md` and
  implemented in `xtask/src/redis_benchmark.rs`.
- Object-storage persistence is built on SlateDB-compatible storage.
- Go end-to-end tests cover Redis protocol behavior across the server boundary.

## P0: Fact Alignment And Benchmark Coherence

- Keep `docs/commands.md`, `docs/redis-benchmark.md`, and
  `xtask/src/redis_benchmark.rs` aligned with the command table.
- Keep the `full` redis-benchmark profile limited to currently implemented
  commands. `FLUSHDB` is setup/cleanup only, not a throughput benchmark.
- Keep the `comparison` redis-benchmark profile on a stable subset that can be
  compared between PR and main branch builds.
- Do not list unsupported commands such as `MGET`, `MSET`, `SUNION`, `SINTER`,
  or `SDIFF` as supported, and do not benchmark them until they are
  implemented.

## P1: Redis-Compatible Command Depth

- Add high-value Redis command options where they fit Nimbis semantics, starting
  with string command options such as `SET` modifiers.
- Expand multi-key string support only when worker routing and storage
  semantics are explicit.
- Add set algebra commands after storage behavior and benchmark coverage are
  defined.
- Keep compatibility notes up to date for intentional gaps.

## P2: Persistence And Recovery Confidence

- Strengthen persistence tests around object-store-backed startup, restart, and
  recovery paths.
- Document operational expectations for supported object storage backends.
- Keep storage benchmarks focused on performance-sensitive paths.

## P3: Operational Readiness

- Improve configuration documentation as runtime and bootstrap-only settings
  evolve.
- Preserve structured logging and telemetry lifecycle ownership.
- Keep CI checks focused on code quality, command compatibility, and benchmark
  signal.
