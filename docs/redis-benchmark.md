# Redis Benchmark

Nimbis can be benchmarked with the upstream `redis-benchmark` command line tool.
Redis itself does not provide a separate benchmark config-file mode: built-in
tests are selected with `-t`, and arbitrary Redis commands can be placed after
the benchmark options.

References:

- [Redis benchmark documentation](https://redis.io/docs/latest/operate/oss_and_stack/management/optimization/benchmarks/)
- [Redis `redis-benchmark.c`](https://github.com/redis/redis/raw/refs/heads/unstable/src/redis-benchmark.c)

## Quick Start

Build and run Nimbis first:

```bash
just build --release
target/release/nimbis
```

Then run the Nimbis redis-benchmark xtask from another terminal:

```bash
just redis-bench
```

For a smaller smoke run:

```bash
N=100 C=2 P=1 just redis-bench
```

For pipelined load:

```bash
N=1000 C=10 P=16 just redis-bench
```

Results are written to `target/redis-benchmark/` and are also printed to stdout.

## Compare Git Branches Locally

Use the branch comparison recipe to build two committed Git refs, benchmark both
release binaries, and print a relative Markdown report in the terminal:

```bash
just redis-bench-compare main HEAD
```

The default refs are `main` and `HEAD`, so the shorter form is equivalent:

```bash
just redis-bench-compare
```

The command runs the same comparison workload against each ref at `P=1` and a
configurable pipeline depth (default `P=50`). Runs are sequential to prevent the
servers from competing for local CPU and I/O resources. Every run uses an
isolated local file store and a dynamically selected loopback port. Readiness is
accepted only after the child reports that it owns the port and answers a real
`PING`. The current worktree is not switched or modified; uncommitted changes
are not part of `HEAD`.

Defaults match one 512-byte pull request benchmark slot (`N=200000`, `C=100`,
`D=512`, and `R=100000`). Existing environment variables and command options can
be used for a smaller local run:

```bash
N=1000 C=10 D=128 R=1000 SEED_N=1000 \
  just redis-bench-compare main feature/my-change --pipeline-depth 16
```

Raw suite output, server logs, and `report.md` are retained below
`target/redis-benchmark-compare/`. Set `COMPARE_OUTPUT_DIR` or pass
`--output-dir` to choose another parent directory. The release build cache is
retained in that parent's `build-cache/` directory to speed up later comparisons.
The temporary source clone and object stores are removed after the command
finishes. Pressing Ctrl-C requests a graceful stop so those temporary resources
and any running child server are cleaned up before exit.

## Pull Request Benchmark CI

Pull request CI compares only the Main and PR Nimbis binaries. Redis, PikiwiDB,
and Kvrocks are not mixed into this noisy change-detection path; cross-database
comparisons should be run as a separate benchmark with separately controlled
environments.

The CI matrix has five independent command shards (`GET/SET`, `HGET/HSET`,
`LPOP/LPUSH`, `SADD/SREM`, and `ZADD/ZREM`), two payload sizes, and three runner
replicas. Different shards run in parallel. Within one shard, commands, pipeline
modes, and Main/PR passes remain sequential so competing servers never share a
runner at the same time.

The isolated command workloads make `D` real for every cell: HGET fixtures use
`D`-byte values, and SADD/SREM/ZADD/ZREM use `D`-byte random members. The other
commands use Redis's built-in payload generation.

Each command/configuration cell uses a balanced four-pass block. Odd replicas
run `Main, PR, PR, Main` (`ABBA`), while even replicas run `PR, Main, Main, PR`
(`BAAB`). Every pass gets a fresh Nimbis process and object store, and both
branches receive the same deterministic random seed from the pinned Redis 8.0.0
benchmark client. This makes the reported value a same-runner relative effect
instead of a comparison of unrelated absolute measurements.

The aggregate report contains:

- `P=1` throughput effects with a ±5% screening materiality band
- pipelined throughput effects with a ±8% screening materiality band
- same-branch duplicate instability lines of 10% and 16%, respectively
- cross-runner effect-range width instability lines of 10 and 16 percentage
  points, respectively
- `P=1` p50 latency as informational evidence
- per-cell median effect, median absolute deviation, range, and duplicate spread

A `candidate regression` requires every stable replica to fall below the
negative materiality boundary; a `candidate improvement` requires every stable
replica to rise above the positive boundary. Either status requests a
confirmation run; neither is a statistical CI gate. Pipelined p50 remains in
the raw JSON but is omitted from the comment because Redis reports
batch/first-read latency rather than independent per-request latency in that
mode. One aggregate comment is updated only after all required shard artifacts
validate successfully. Raw output, seeds, logs, binary hashes, runner metadata,
and tool versions remain downloadable.

The same-branch duplicate-spread veto triggers when the spread exceeds 10% for
`P=1` or 16% for the pipelined measurement. The cross-runner dispersion veto
triggers when the effect range width exceeds twice the materiality boundary:
10 percentage points for `P=1` and 16 for the pipelined measurement. Both
quality vetoes are evaluated before materiality classification. These are
conservative screening heuristics. They must be calibrated with repeated A/A
blocks before any benchmark status is promoted to a required CI gate. A mixed
result is reported as inconclusive; observations inside the screening lines do
not establish performance equivalence.

## Configuration

The xtask is configured with environment variables or equivalent CLI flags.

```bash
HOST=127.0.0.1 \
PORT=6379 \
N=500000 \
C=50 \
D=128 \
P=1 \
R=100000 \
THREADS=4 \
CSV=1 \
OUTPUT_DIR=target/redis-benchmark \
just redis-bench
```

Supported environment variables:

- `HOST`: Redis host, default `127.0.0.1`
- `PORT`: Redis port, default `6379`
- `N`: request count per benchmark, default `500000`
- `C`: concurrent clients, default `50`
- `D`: payload size for SET-like benchmark values, default `128`
- `P`: pipeline depth, default `1`
- `R`: random key space for `__rand_int__`, default `100000`
- `THREADS`: optional `redis-benchmark --threads` value
- `CSV`: set to `1` or `true` to use `--csv`; otherwise the xtask uses `-q`
- `OUTPUT_DIR`: result directory, default `target/redis-benchmark`
- `SEED_N`: setup request count for seeded random data, default matches `N`
- `REDIS_BENCHMARK`: override benchmark binary name/path
- `REDIS_CLI`: override cli binary name/path

The same values can be passed as CLI flags:

```bash
cargo xtask redis-benchmark --n 10000 --c 100 --p 16 --threads 4
```

The comparison profile can also isolate one command. `--seed` requires a Redis
8 or newer benchmark client, accepts Redis 8's integer range up to `2147483647`,
and makes the random-key stream deterministic; `--settle-millis` controls the
pause between fixture setup and measurement.

```bash
cargo xtask redis-benchmark --profile comparison --command get \
  --seed 277000 --settle-millis 1000
```

Extra arguments for `redis-benchmark` can be passed after `--` and are forwarded
to every benchmark invocation.

The default command profile is `full`, which covers the currently implemented
Nimbis command table from [Commands](commands.md). `FLUSHDB` is used only for
setup and cleanup isolation, not as a throughput benchmark. Benchmark CI uses
`--profile comparison` for the main-vs-PR comparison so the main branch can be
benchmarked before it has newly added commands from a PR.

## Built-In Coverage

The xtask intentionally does not run the full default Redis benchmark suite.
Redis includes tests for commands that Nimbis does not currently implement, so
the xtask keeps an explicit allowlist.

Built-in Redis tests enabled for Nimbis:

- `ping`
- `set`
- `get`
- `incr`
- `lpush`
- `rpush`
- `lpop`
- `rpop`
- `sadd`
- `hset`
- `zadd`
- `lrange` (runs Redis's `LRANGE_100`, `LRANGE_300`, `LRANGE_500`, and
  `LRANGE_600` cases after their built-in `LPUSH` setup)

Built-in Redis tests skipped because Nimbis does not currently implement the
commands:

- `spop`
- `zpopmin`
- `xadd`

To focus on the large list-range cases against a running release server, use:

```bash
redis-benchmark -h 127.0.0.1 -p 6379 -n 10000 -c 50 -d 128 -P 1 \
  -t lrange_300,lrange_500,lrange_600
```

## Custom Command Coverage

Commands not covered by Redis built-ins are benchmarked by passing the command
directly to `redis-benchmark`.

Covered command groups:

- String: `DECR`, `APPEND`
- Typed key lifecycle: `DEL STRING key [key ...]`,
  `EXISTS STRING key [key ...]`, `EXPIRE STRING key seconds`, and
  `TTL STRING key`
- Hash: `HDEL`, `HGET`, `HLEN`, `HMGET`, `HGETALL`
- List: `LLEN`
- Set: `SMEMBERS`, `SISMEMBER`, `SREM`, `SCARD`
- Sorted set: `ZRANGE`, `ZSCORE`, `ZREM`, `ZCARD`
- Control smoke: `HELLO 2`, `CONFIG GET *`, `CLIENT ID`

The lifecycle workload uses the `STRING` type because its random fixtures are
seeded with `SET`. Fixed List, Set, and ZSet fixtures are reset with the matching
typed `DEL` form before they are seeded.

`FLUSHDB` is used only for setup and cleanup isolation. It is not included in
throughput comparisons.

The `comparison` profile is intentionally smaller than `full`. It benchmarks:

- Built-in tests: `set`, `get`, `hset`, `lpush`, `lpop`, `sadd`, `zadd`
- Custom commands: `HGET`, `SREM`, `ZREM`

## Notes

- The xtask requires both `redis-benchmark` and `redis-cli` in `PATH`.
- For `just redis-bench`, the target Nimbis server must already be running. The
  branch comparison command builds and starts both servers automatically.
- Each suite uses stable key prefixes to reduce cross-test pollution.
- Destructive commands such as `DEL STRING`, `HDEL`, `SREM`, and `ZREM` are
  seeded before benchmarking so they do not benchmark an entirely cold miss
  path.
- `__rand_int__` is used with `-r` for random-key workloads.
