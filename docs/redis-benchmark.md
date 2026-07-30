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
SEED=1 \
WARMUP_N=1000 \
SETTLE_MS=5000 \
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
- `SEED`: deterministic `redis-benchmark` random seed, default `1`, maximum
  `2147483647`
- `WARMUP_N`: request count before each measured benchmark, default `1000`;
  set to `0` to disable
- `SETTLE_MS`: quiet delay after setup and selected write-heavy phases, default
  `5000`; set to `0` to disable
- `THREADS`: optional `redis-benchmark --threads` value
- `CSV`: set to `1` or `true` to use `--csv`; otherwise the xtask uses `-q`
- `OUTPUT_DIR`: result directory, default `target/redis-benchmark`
- `SEED_N`: setup request count for seeded random data, default matches `N`
- `REDIS_BENCHMARK`: override benchmark binary name/path
- `REDIS_CLI`: override cli binary name/path

The same values can be passed as CLI flags:

```bash
cargo xtask redis-benchmark \
  --n 10000 \
  --c 100 \
  --p 16 \
  --seed 42 \
  --warmup-n 1000 \
  --settle-ms 5000 \
  --threads 4
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

Built-in Redis tests skipped because Nimbis does not currently implement the
commands:

- `spop`
- `zpopmin`
- `xadd`

Redis `LRANGE` built-ins are skipped from the Nimbis benchmark because
`redis-benchmark -t lrange` expands into the larger `LRANGE_300`,
`LRANGE_500`, and `LRANGE_600` cases. The `full` profile covers Nimbis
`LRANGE` with a direct custom command instead.

- `lrange`
- `lrange_100`
- `lrange_300`
- `lrange_500`
- `lrange_600`

## Custom Command Coverage

Commands not covered by Redis built-ins are benchmarked by passing the command
directly to `redis-benchmark`.

Covered command groups:

- String/generic: `DEL`, `EXISTS`, `DECR`, `APPEND`
- Hash: `HDEL`, `HGET`, `HLEN`, `HMGET`, `HGETALL`
- List: `LLEN`, `LRANGE`
- Set: `SMEMBERS`, `SISMEMBER`, `SREM`, `SCARD`
- Sorted set: `ZRANGE`, `ZSCORE`, `ZREM`, `ZCARD`
- TTL: `EXPIRE`, `TTL`
- Control smoke: `HELLO 2`, `CONFIG GET *`, `CLIENT ID`

`FLUSHDB` is used only for setup and cleanup isolation. It is not included in
throughput comparisons.

The `comparison` profile is intentionally smaller than `full`. It benchmarks:

- Built-in tests: `set`, `get`, `hset`, `lpush`, `lpop`, `sadd`, `zadd`
- Custom commands: `HGET`, `SREM`, `ZREM`

It only pre-populates the random data needed by `SREM` and `ZREM`; fixed HGET
data is created with `HSET`. The full profile keeps the broader setup required
by its destructive command coverage.

## CI Comparison Stability

The pull request workflow compares Main and PR on the same hosted runner. Each
matrix slot runs two paired rounds in opposite order (`Main → PR`, then
`PR → Main`), with a fresh object-store directory for every process. Both sides
of a pair use the same random seed. Three matrix slots are aggregated into six
paired samples for each payload size. The workflow also:

- waits for `PING` instead of relying on a fixed startup sleep
- prebuilds the xtask so compilation cannot overlap a measurement
- warms up built-in and read-only custom commands before recording them;
  destructive custom commands skip warmup so their seeded state stays intact
- leaves 20-second quiet periods after setup and write-heavy phases to reduce
  overlap with background flush and compaction work
- reports the median paired change, median absolute deviation (MAD), and the
  observed min/max range
- retains raw round outputs, server logs, and runner metadata as a 14-day
  workflow artifact
- cancels superseded PR runs and verifies the current head SHA before updating
  benchmark comments

The quiet period is a heuristic, not proof that SlateDB is fully quiescent.
Raw RPS from separate hosted runners can still differ because their CPU
allocation is not identical; the same-runner paired percentage and its spread
are the primary regression signals. Redis, PikiwiDB, and Kvrocks RPS values are
unpaired medians across the available runners and are shown only as
informational references. An unavailable external baseline does not suppress
the Main-vs-PR report.

## Notes

- The xtask requires both `redis-benchmark` and `redis-cli` in `PATH`.
- Deterministic seeds require `redis-benchmark` 7.2 or newer. CI extracts a
  pinned 7.2.0 benchmark client while leaving the runner's Redis server package
  unchanged.
- The target Nimbis server must already be running.
- Each suite uses stable key prefixes to reduce cross-test pollution.
- Destructive commands such as `DEL`, `HDEL`, `SREM`, and `ZREM` are seeded
  before benchmarking so they do not benchmark an entirely cold miss path.
- `__rand_int__` is used with `-r` for random-key workloads.
