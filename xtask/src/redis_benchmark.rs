use std::env;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::iter::repeat_n;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use clap::Args as ClapArgs;
use clap::ValueEnum;

use crate::benchmarks;
use crate::write_stdout_line;

const BUILTIN_SUPPORTED: &str = "ping,set,get,incr,lpush,rpush,lpop,rpop,sadd,hset,zadd,lrange";
const RANDOM_TOKEN: &str = "__rand_int__";
pub(crate) const MAX_REDIS_RANDOM_SEED: u64 = i32::MAX as u64;
pub(crate) const DEFAULT_COMPARISON_SEED: u64 = 279_000;
pub(crate) const COMPARISON_PROFILE_COMMANDS: &[&str] = &[
	"GET", "HGET", "HSET", "LPOP", "LPUSH", "SADD", "SET", "SREM", "ZADD", "ZREM",
];

#[derive(ClapArgs, Debug, Default)]
pub struct Args {
	/// Redis host.
	#[arg(long)]
	pub host: Option<String>,

	/// Redis port.
	#[arg(long)]
	pub port: Option<u16>,

	/// Request count per benchmark.
	#[arg(long = "n")]
	pub requests: Option<u64>,

	/// Concurrent clients.
	#[arg(long = "c")]
	pub clients: Option<u64>,

	/// Payload size for SET-like benchmark values.
	#[arg(long = "d")]
	pub data_size: Option<u64>,

	/// Pipeline depth.
	#[arg(long = "p")]
	pub pipeline: Option<u64>,

	/// Random key space for __rand_int__.
	#[arg(long = "r")]
	pub random_keyspace: Option<u64>,

	/// Optional redis-benchmark --threads value.
	#[arg(long)]
	pub threads: Option<u64>,

	/// Use redis-benchmark --csv output instead of -q.
	#[arg(long)]
	pub csv: bool,

	/// Ignore CSV=1 and force parseable quiet output for internal callers.
	#[arg(skip)]
	#[doc(hidden)]
	pub force_quiet: bool,

	/// Result directory.
	#[arg(long)]
	pub output_dir: Option<String>,

	/// Setup request count for seeded random data.
	#[arg(long = "seed-n")]
	pub seed_requests: Option<u64>,

	/// Comparison-profile command to benchmark in isolation.
	#[arg(long, value_enum)]
	pub command: Option<ComparisonCommand>,

	/// Deterministic redis-benchmark random seed (Redis 8 or newer). Defaults
	/// to 279000 for the comparison profile.
	#[arg(long)]
	pub seed: Option<u64>,

	/// Milliseconds to wait after seeding fixtures before measurement.
	#[arg(long, default_value = "0")]
	pub settle_millis: Option<u64>,

	/// Override redis-benchmark binary name/path.
	#[arg(long)]
	pub redis_benchmark: Option<String>,

	/// Override redis-cli binary name/path.
	#[arg(long)]
	pub redis_cli: Option<String>,

	/// Extra arguments forwarded to every redis-benchmark invocation.
	#[arg(last = true)]
	pub extra_args: Vec<String>,

	/// Benchmark command profile.
	#[arg(long, value_enum, default_value_t = Profile::Full)]
	pub profile: Profile,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum Profile {
	/// Run the full Nimbis-supported command coverage.
	#[default]
	Full,
	/// Run only the legacy common command set used for CI comparisons.
	Comparison,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ComparisonCommand {
	Get,
	Set,
	Hget,
	Hset,
	Lpush,
	Lpop,
	Sadd,
	Srem,
	Zadd,
	Zrem,
	#[value(name = "lpop-count-1")]
	LpopCount1,
	#[value(name = "lpop-count-2")]
	LpopCount2,
	#[value(name = "lpop-count-32")]
	LpopCount32,
	#[value(name = "lpop-count-256")]
	LpopCount256,
	#[value(name = "rpop-count-1")]
	RpopCount1,
	#[value(name = "rpop-count-2")]
	RpopCount2,
	#[value(name = "rpop-count-32")]
	RpopCount32,
	#[value(name = "rpop-count-256")]
	RpopCount256,
}

impl ComparisonCommand {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Get => "GET",
			Self::Set => "SET",
			Self::Hget => "HGET",
			Self::Hset => "HSET",
			Self::Lpush => "LPUSH",
			Self::Lpop => "LPOP",
			Self::Sadd => "SADD",
			Self::Srem => "SREM",
			Self::Zadd => "ZADD",
			Self::Zrem => "ZREM",
			Self::LpopCount1 => "LPOP_COUNT_1",
			Self::LpopCount2 => "LPOP_COUNT_2",
			Self::LpopCount32 => "LPOP_COUNT_32",
			Self::LpopCount256 => "LPOP_COUNT_256",
			Self::RpopCount1 => "RPOP_COUNT_1",
			Self::RpopCount2 => "RPOP_COUNT_2",
			Self::RpopCount32 => "RPOP_COUNT_32",
			Self::RpopCount256 => "RPOP_COUNT_256",
		}
	}

	fn label(self) -> &'static str {
		match self {
			Self::Get => "get",
			Self::Set => "set",
			Self::Hget => "hget",
			Self::Hset => "hset",
			Self::Lpush => "lpush",
			Self::Lpop => "lpop",
			Self::Sadd => "sadd",
			Self::Srem => "srem",
			Self::Zadd => "zadd",
			Self::Zrem => "zrem",
			Self::LpopCount1 => "lpop_count_1",
			Self::LpopCount2 => "lpop_count_2",
			Self::LpopCount32 => "lpop_count_32",
			Self::LpopCount256 => "lpop_count_256",
			Self::RpopCount1 => "rpop_count_1",
			Self::RpopCount2 => "rpop_count_2",
			Self::RpopCount32 => "rpop_count_32",
			Self::RpopCount256 => "rpop_count_256",
		}
	}

	pub(crate) fn counted_pop(self) -> Option<(&'static str, u64)> {
		match self {
			Self::LpopCount1 => Some(("LPOP", 1)),
			Self::LpopCount2 => Some(("LPOP", 2)),
			Self::LpopCount32 => Some(("LPOP", 32)),
			Self::LpopCount256 => Some(("LPOP", 256)),
			Self::RpopCount1 => Some(("RPOP", 1)),
			Self::RpopCount2 => Some(("RPOP", 2)),
			Self::RpopCount32 => Some(("RPOP", 32)),
			Self::RpopCount256 => Some(("RPOP", 256)),
			_ => None,
		}
	}
}

#[derive(Clone, Debug)]
struct Config {
	host: String,
	port: u16,
	requests: u64,
	clients: u64,
	data_size: u64,
	pipeline: u64,
	random_keyspace: u64,
	threads: Option<u64>,
	csv: bool,
	output_dir: PathBuf,
	seed_requests: u64,
	command: Option<ComparisonCommand>,
	seed: Option<u64>,
	settle_millis: u64,
	redis_benchmark: String,
	redis_cli: String,
	extra_args: Vec<String>,
	profile: Profile,
}

impl Config {
	fn from_args(args: &Args, workspace_root: &Path) -> Result<Self, String> {
		let requests = option_or_env_u64(args.requests, "N", 500000)?;
		let output_dir = option_or_env_string(
			args.output_dir.as_deref(),
			"OUTPUT_DIR",
			"target/redis-benchmark",
		);
		let output_dir = resolve_output_dir(workspace_root, &output_dir);

		let config = Self {
			host: option_or_env_string(args.host.as_deref(), "HOST", "127.0.0.1"),
			port: option_or_env_u16(args.port, "PORT", 6379)?,
			requests,
			clients: option_or_env_u64(args.clients, "C", 50)?,
			data_size: option_or_env_u64(args.data_size, "D", 128)?,
			pipeline: option_or_env_u64(args.pipeline, "P", 1)?,
			random_keyspace: option_or_env_u64(args.random_keyspace, "R", 100000)?,
			threads: option_or_env_optional_u64(args.threads, "THREADS")?,
			csv: !args.force_quiet && (args.csv || env_bool("CSV")),
			output_dir,
			seed_requests: option_or_env_u64(args.seed_requests, "SEED_N", requests)?,
			command: args.command,
			seed: args.seed.or_else(|| {
				(args.profile == Profile::Comparison).then_some(DEFAULT_COMPARISON_SEED)
			}),
			settle_millis: args.settle_millis.unwrap_or(0),
			redis_benchmark: option_or_env_string(
				args.redis_benchmark.as_deref(),
				"REDIS_BENCHMARK",
				"redis-benchmark",
			),
			redis_cli: option_or_env_string(args.redis_cli.as_deref(), "REDIS_CLI", "redis-cli"),
			extra_args: args.extra_args.clone(),
			profile: args.profile,
		};
		if config.command.is_some() && config.profile != Profile::Comparison {
			return Err("--command requires --profile comparison".into());
		}
		if config.seed_requests == 0 {
			return Err("--seed-n must be greater than zero".into());
		}
		if config.seed.is_some_and(|seed| seed > MAX_REDIS_RANDOM_SEED) {
			return Err(format!(
				"--seed must not exceed {MAX_REDIS_RANDOM_SEED} for Redis 8 compatibility"
			));
		}
		if config
			.command
			.is_some_and(|command| command.counted_pop().is_some())
		{
			validate_counted_pop_config(&config)?;
		}
		Ok(config)
	}

	fn benchmark_base_args(&self) -> Vec<String> {
		let mut args = vec![
			"-h".to_string(),
			self.host.clone(),
			"-p".to_string(),
			self.port.to_string(),
			"-n".to_string(),
			self.requests.to_string(),
			"-c".to_string(),
			self.clients.to_string(),
			"-d".to_string(),
			self.data_size.to_string(),
			"-r".to_string(),
			self.random_keyspace.to_string(),
			"-P".to_string(),
			self.pipeline.to_string(),
		];
		if let Some(seed) = self.seed {
			args.push("--seed".to_string());
			args.push(seed.to_string());
		}

		if let Some(threads) = self.threads {
			args.push("--threads".to_string());
			args.push(threads.to_string());
		}

		if self.csv {
			args.push("--csv".to_string());
		} else {
			args.push("-q".to_string());
		}

		args
	}

	fn output_ext(&self) -> &'static str {
		if self.csv { "csv" } else { "txt" }
	}
}

pub fn run(args: Args, workspace_root: &Path) -> Result<(), String> {
	let config = Config::from_args(&args, workspace_root)?;
	let runner = ProcessRunner;
	run_with_runner(&config, &runner)
}

trait Runner {
	fn run_status(&self, program: &str, args: &[String]) -> Result<(), String>;
	fn run_output(&self, program: &str, args: &[String]) -> Result<String, String>;
	fn run_streaming_output(
		&self,
		program: &str,
		args: &[String],
		file: &Path,
	) -> Result<(), String>;
}

fn run_with_runner<R: Runner>(config: &Config, runner: &R) -> Result<(), String> {
	require_cmd(&config.redis_benchmark)?;
	require_cmd(&config.redis_cli)?;
	fs::create_dir_all(&config.output_dir).map_err(|error| error.to_string())?;

	redis_cli(config, runner, &["PING"])?;

	write_stdout_line("Running Nimbis redis-benchmark suite")?;
	write_stdout_line(&format!(
		"host={} port={} n={} clients={} data_size={} pipeline={} random_keyspace={} output={}",
		config.host,
		config.port,
		config.requests,
		config.clients,
		config.data_size,
		config.pipeline,
		config.random_keyspace,
		config.output_dir.display()
	))?;
	write_stdout_line("")?;

	redis_cli(config, runner, &["FLUSHDB"])?;
	if let Some(command) = config.command {
		if let Some((direction, count)) = command.counted_pop() {
			run_counted_pop(config, runner, command.label(), direction, count)?;
		} else {
			seed_comparison_command(config, runner, command)?;
			settle_after_seed(config);
			run_comparison_command(config, runner, command)?;
		}
	} else {
		seed_fixed_data(config, runner)?;
		seed_random_data(config, runner)?;
		settle_after_seed(config);
		match config.profile {
			Profile::Full => {
				run_builtin_suite(config, runner)?;
				run_custom_suite(config, runner)?;
				run_control_smoke_suite(config, runner)?;
			}
			Profile::Comparison => run_comparison_suite(config, runner)?,
		}
	}

	write_stdout_line("")?;
	write_stdout_line(&format!(
		"redis-benchmark results written to {}",
		config.output_dir.display()
	))?;
	Ok(())
}

fn seed_comparison_command<R: Runner>(
	config: &Config,
	runner: &R,
	command: ComparisonCommand,
) -> Result<(), String> {
	match command {
		ComparisonCommand::Get => seed_benchmark(config, runner, &["-t", "set"]),
		ComparisonCommand::Hget => {
			let value = fixed_payload(config.data_size)?;
			redis_cli(config, runner, &["HSET", "bench:hash", "field1", &value])
		}
		ComparisonCommand::Lpop => seed_benchmark(config, runner, &["-t", "lpush"]),
		ComparisonCommand::Srem => {
			let member = random_payload(config.data_size)?;
			seed_benchmark(config, runner, &["SADD", "bench:set:srem", &member])
		}
		ComparisonCommand::Zrem => {
			let member = random_payload(config.data_size)?;
			seed_benchmark(config, runner, &["ZADD", "bench:zset:zrem", "1", &member])
		}
		ComparisonCommand::Set
		| ComparisonCommand::Hset
		| ComparisonCommand::Lpush
		| ComparisonCommand::Sadd
		| ComparisonCommand::Zadd => Ok(()),
		_ => Err("counted pops require their validated fixture path".into()),
	}
}

fn run_comparison_command<R: Runner>(
	config: &Config,
	runner: &R,
	command: ComparisonCommand,
) -> Result<(), String> {
	let label = command.label();
	match command {
		ComparisonCommand::Get
		| ComparisonCommand::Set
		| ComparisonCommand::Hset
		| ComparisonCommand::Lpush
		| ComparisonCommand::Lpop => run_benchmark(config, runner, label, &["-t", label]),
		ComparisonCommand::Hget => {
			run_benchmark(config, runner, label, &["HGET", "bench:hash", "field1"])
		}
		ComparisonCommand::Sadd => {
			let member = random_payload(config.data_size)?;
			run_benchmark(config, runner, label, &["SADD", "bench:set:sadd", &member])
		}
		ComparisonCommand::Srem => {
			let member = random_payload(config.data_size)?;
			run_benchmark(config, runner, label, &["SREM", "bench:set:srem", &member])
		}
		ComparisonCommand::Zadd => {
			let member = random_payload(config.data_size)?;
			run_benchmark(
				config,
				runner,
				label,
				&["ZADD", "bench:zset:zadd", RANDOM_TOKEN, &member],
			)
		}
		ComparisonCommand::Zrem => {
			let member = random_payload(config.data_size)?;
			run_benchmark(config, runner, label, &["ZREM", "bench:zset:zrem", &member])
		}
		_ => Err("counted pops require their validated fixture path".into()),
	}
}

fn validate_counted_pop_config(config: &Config) -> Result<(), String> {
	let batch = config
		.clients
		.checked_mul(config.pipeline)
		.filter(|batch| *batch > 0)
		.ok_or_else(|| "counted pops require positive C and P without overflow".to_string())?;
	if config.requests == 0 || !config.requests.is_multiple_of(batch) {
		return Err("counted pops require N to be a positive multiple of C * P".into());
	}
	if config.requests > MAX_REDIS_RANDOM_SEED - 2 || config.data_size == 0 {
		return Err("counted pops require N + 2 <= i32::MAX and D > 0".into());
	}
	if config.csv || !config.extra_args.is_empty() {
		return Err(
			"counted pops require quiet output and do not allow extra benchmark arguments".into(),
		);
	}
	Ok(())
}

fn run_counted_pop<R: Runner>(
	config: &Config,
	runner: &R,
	label: &str,
	direction: &str,
	count: u64,
) -> Result<(), String> {
	validate_counted_pop_config(config)?;
	let key = "bench:list:counted-pop";
	let value = fixed_payload(config.data_size)?;
	let count_arg = count.to_string();
	let seed = Config {
		seed_requests: config.requests + 2,
		pipeline: 1,
		..config.clone()
	};
	let mut push = vec!["RPUSH", key];
	push.extend(repeat_n(value.as_str(), count as usize));
	seed_benchmark(&seed, runner, &push)?;
	let seeded_elements = (config.requests + 2) * count;
	check_list_len(config, runner, key, seeded_elements)?;
	let popped = redis_cli_output(config, runner, &["--json", direction, key, &count_arg])?;
	let popped: Vec<String> = serde_json::from_str(&popped)
		.map_err(|error| format!("Invalid {direction} count preflight response: {error}"))?;
	if popped.len() != count as usize || popped.iter().any(|item| item != &value) {
		return Err(format!(
			"{direction} {count} preflight did not return {count} seeded values"
		));
	}
	check_list_len(config, runner, key, (config.requests + 1) * count)?;
	settle_after_seed(config);
	let started = Instant::now();
	run_benchmark(config, runner, label, &[direction, key, &count_arg])?;
	let wall_seconds = started.elapsed().as_secs_f64();
	check_list_len(config, runner, key, count)?;

	let output = config.output_dir.join(format!("{label}.txt"));
	let raw = fs::read_to_string(&output).map_err(|error| error.to_string())?;
	let result = benchmarks::parse_benchmark(&raw)
		.remove(direction)
		.filter(|result| result.rps.is_finite() && result.rps > 0.0)
		.ok_or_else(|| format!("No positive {direction} throughput in {}", output.display()))?;
	let validation = serde_json::json!({
		"command": direction,
		"count": count,
		"requests": config.requests,
		"clients": config.clients,
		"pipeline": config.pipeline,
		"element_bytes": config.data_size,
		"seeded_elements": seeded_elements,
		"preflight_elements": count,
		"measured_elements": config.requests * count,
		"remaining_elements": count,
		"requests_per_second": result.rps,
		"elements_per_second": result.rps * count as f64,
		"measurement_seconds_from_rps": config.requests as f64 / result.rps,
		"benchmark_process_wall_seconds": wall_seconds,
	});
	let validation_path = config.output_dir.join(format!("{label}-validation.json"));
	fs::write(&validation_path, format!("{validation:#}\n"))
		.map_err(|error| format!("Failed to write {}: {error}", validation_path.display()))?;
	write_stdout_line(&format!(
		"Verified {direction} count={count}: {} full requests, {count} elements remain",
		config.requests
	))
}

fn check_list_len<R: Runner>(
	config: &Config,
	runner: &R,
	key: &str,
	expected: u64,
) -> Result<(), String> {
	let output = redis_cli_output(config, runner, &["--raw", "LLEN", key])?;
	if output.trim().parse::<u64>() != Ok(expected) {
		return Err(format!(
			"counted-pop LLEN expected {expected}, received {}",
			output.trim()
		));
	}
	Ok(())
}

fn redis_cli_output<R: Runner>(
	config: &Config,
	runner: &R,
	command_args: &[&str],
) -> Result<String, String> {
	let mut args = vec![
		"-h".into(),
		config.host.clone(),
		"-p".into(),
		config.port.to_string(),
		"-e".into(),
	];
	args.extend(command_args.iter().map(|arg| (*arg).to_string()));
	runner.run_output(&config.redis_cli, &args)
}

fn fixed_payload(data_size: u64) -> Result<String, String> {
	let data_size = usize::try_from(data_size)
		.map_err(|_| format!("data size {data_size} does not fit in memory"))?;
	Ok("x".repeat(data_size))
}

fn random_payload(data_size: u64) -> Result<String, String> {
	let data_size = usize::try_from(data_size)
		.map_err(|_| format!("data size {data_size} does not fit in memory"))?;
	if data_size < RANDOM_TOKEN.len() {
		return Err(format!(
			"data size must be at least {} for random member workloads",
			RANDOM_TOKEN.len()
		));
	}
	Ok(format!(
		"{}{}",
		"x".repeat(data_size - RANDOM_TOKEN.len()),
		RANDOM_TOKEN
	))
}

fn settle_after_seed(config: &Config) {
	if config.settle_millis > 0 {
		thread::sleep(Duration::from_millis(config.settle_millis));
	}
}

fn seed_fixed_data<R: Runner>(config: &Config, runner: &R) -> Result<(), String> {
	redis_cli(config, runner, &["SET", "bench:string:get", "value"])?;
	redis_cli(config, runner, &["SET", "bench:string:ttl", "value"])?;
	redis_cli(
		config,
		runner,
		&[
			"HSET",
			"bench:hash",
			"field1",
			"value1",
			"field2",
			"value2",
			"field3",
			"value3",
		],
	)?;
	redis_cli(config, runner, &["DEL", "LIST", "bench:list"])?;
	redis_cli(
		config,
		runner,
		&[
			"RPUSH",
			"bench:list",
			"a",
			"b",
			"c",
			"d",
			"e",
			"f",
			"g",
			"h",
			"i",
			"j",
		],
	)?;
	redis_cli(
		config,
		runner,
		&["DEL", "SET", "bench:set:a", "bench:set:b"],
	)?;
	redis_cli(config, runner, &["SADD", "bench:set:a", "a", "b", "c"])?;
	redis_cli(config, runner, &["SADD", "bench:set:b", "b", "c", "d"])?;
	redis_cli(config, runner, &["DEL", "ZSET", "bench:zset"])?;
	redis_cli(
		config,
		runner,
		&[
			"ZADD",
			"bench:zset",
			"1",
			"one",
			"2",
			"two",
			"3",
			"three",
			"4",
			"four",
		],
	)?;
	Ok(())
}

fn seed_random_data<R: Runner>(config: &Config, runner: &R) -> Result<(), String> {
	seed_benchmark(
		config,
		runner,
		&["SET", "bench:string:a:__rand_int__", "value-a"],
	)?;
	seed_benchmark(
		config,
		runner,
		&["SET", "bench:string:b:__rand_int__", "value-b"],
	)?;
	seed_benchmark(
		config,
		runner,
		&["SET", "bench:string:del:a:__rand_int__", "value-a"],
	)?;
	seed_benchmark(
		config,
		runner,
		&["SET", "bench:string:del:b:__rand_int__", "value-b"],
	)?;
	seed_benchmark(
		config,
		runner,
		&["SET", "bench:string:decr:__rand_int__", "1000000"],
	)?;
	seed_benchmark(
		config,
		runner,
		&["HSET", "bench:hash:hdel", "field:__rand_int__", "value"],
	)?;
	seed_benchmark(
		config,
		runner,
		&["SADD", "bench:set:srem", "member:__rand_int__"],
	)?;
	seed_benchmark(
		config,
		runner,
		&[
			"ZADD",
			"bench:zset:zrem",
			"__rand_int__",
			"member:__rand_int__",
		],
	)?;
	seed_benchmark(
		config,
		runner,
		&["SET", "bench:string:expire:__rand_int__", "value"],
	)?;
	Ok(())
}

fn run_builtin_suite<R: Runner>(config: &Config, runner: &R) -> Result<(), String> {
	run_benchmark(
		config,
		runner,
		"builtin_supported",
		&["-t", BUILTIN_SUPPORTED],
	)
}

fn run_comparison_suite<R: Runner>(config: &Config, runner: &R) -> Result<(), String> {
	run_benchmark(
		config,
		runner,
		"builtin_comparison",
		&["-t", "set,get,hset,lpush,lpop,sadd,zadd"],
	)?;

	let benchmarks: &[(&str, &[&str])] = &[
		("hget", &["HGET", "bench:hash", "field1"]),
		("srem", &["SREM", "bench:set:srem", "member:__rand_int__"]),
		("zrem", &["ZREM", "bench:zset:zrem", "member:__rand_int__"]),
	];
	for (label, args) in benchmarks {
		run_benchmark(config, runner, label, args)?;
	}
	Ok(())
}

fn run_custom_suite<R: Runner>(config: &Config, runner: &R) -> Result<(), String> {
	let benchmarks: &[(&str, &[&str])] = &[
		(
			"del_multi_key",
			&[
				"DEL",
				"STRING",
				"bench:string:del:a:__rand_int__",
				"bench:string:del:b:__rand_int__",
			],
		),
		(
			"exists_multi_key",
			&[
				"EXISTS",
				"STRING",
				"bench:string:a:__rand_int__",
				"bench:string:b:__rand_int__",
				"bench:string:missing:__rand_int__",
			],
		),
		("decr", &["DECR", "bench:string:decr:__rand_int__"]),
		(
			"append",
			&["APPEND", "bench:string:append:__rand_int__", "value"],
		),
		("hdel", &["HDEL", "bench:hash:hdel", "field:__rand_int__"]),
		("hget", &["HGET", "bench:hash", "field1"]),
		("hlen", &["HLEN", "bench:hash"]),
		(
			"hmget",
			&["HMGET", "bench:hash", "field1", "field2", "missing"],
		),
		("hgetall", &["HGETALL", "bench:hash"]),
		("llen", &["LLEN", "bench:list"]),
		("smembers", &["SMEMBERS", "bench:set:a"]),
		("sismember", &["SISMEMBER", "bench:set:a", "a"]),
		("srem", &["SREM", "bench:set:srem", "member:__rand_int__"]),
		("scard", &["SCARD", "bench:set:a"]),
		("zrange", &["ZRANGE", "bench:zset", "0", "-1"]),
		("zscore", &["ZSCORE", "bench:zset", "one"]),
		("zrem", &["ZREM", "bench:zset:zrem", "member:__rand_int__"]),
		("zcard", &["ZCARD", "bench:zset"]),
		(
			"expire",
			&[
				"EXPIRE",
				"STRING",
				"bench:string:expire:__rand_int__",
				"300",
			],
		),
		("ttl", &["TTL", "STRING", "bench:string:ttl"]),
	];

	for (label, args) in benchmarks {
		run_benchmark(config, runner, label, args)?;
	}
	Ok(())
}

fn run_control_smoke_suite<R: Runner>(config: &Config, runner: &R) -> Result<(), String> {
	run_benchmark(config, runner, "hello_2", &["HELLO", "2"])?;
	run_benchmark(config, runner, "config_get_all", &["CONFIG", "GET", "*"])?;
	run_benchmark(config, runner, "client_id", &["CLIENT", "ID"])?;
	Ok(())
}

fn seed_benchmark<R: Runner>(
	config: &Config,
	runner: &R,
	command_args: &[&str],
) -> Result<(), String> {
	let mut args = vec![
		"-h".to_string(),
		config.host.clone(),
		"-p".to_string(),
		config.port.to_string(),
		"-n".to_string(),
		config.seed_requests.to_string(),
		"-c".to_string(),
		config.clients.to_string(),
		"-d".to_string(),
		config.data_size.to_string(),
		"-r".to_string(),
		config.random_keyspace.to_string(),
		"-P".to_string(),
		config.pipeline.to_string(),
	];
	if let Some(seed) = config.seed {
		args.push("--seed".to_string());
		args.push(seed.to_string());
	}
	args.extend(config.extra_args.clone());
	args.extend(command_args.iter().map(|arg| (*arg).to_string()));
	runner.run_status(&config.redis_benchmark, &args)
}

fn run_benchmark<R: Runner>(
	config: &Config,
	runner: &R,
	label: &str,
	command_args: &[&str],
) -> Result<(), String> {
	write_stdout_line(&format!("==> {label}"))?;

	let mut args = config.benchmark_base_args();
	args.extend(config.extra_args.clone());
	args.extend(command_args.iter().map(|arg| (*arg).to_string()));

	let file = config
		.output_dir
		.join(format!("{}.{}", slugify(label), config.output_ext()));
	runner.run_streaming_output(&config.redis_benchmark, &args, &file)
}

fn redis_cli<R: Runner>(config: &Config, runner: &R, command_args: &[&str]) -> Result<(), String> {
	let mut args = vec![
		"-h".to_string(),
		config.host.clone(),
		"-p".to_string(),
		config.port.to_string(),
		"-e".to_string(),
	];
	args.extend(command_args.iter().map(|arg| (*arg).to_string()));
	runner.run_status(&config.redis_cli, &args)
}

struct ProcessRunner;

impl Runner for ProcessRunner {
	fn run_status(&self, program: &str, args: &[String]) -> Result<(), String> {
		self.run_output(program, args).map(|_| ())
	}

	fn run_output(&self, program: &str, args: &[String]) -> Result<String, String> {
		let output = Command::new(program)
			.args(args)
			.output()
			.map_err(|error| format!("Failed to run {program}: {error}"))?;
		if output.status.success() {
			String::from_utf8(output.stdout)
				.map_err(|error| format!("Invalid {program} output: {error}"))
		} else {
			let stderr = String::from_utf8_lossy(&output.stderr);
			let stdout = String::from_utf8_lossy(&output.stdout);
			Err(format!(
				"{program} exited with status {}: {stderr}{stdout}",
				output.status
			))
		}
	}

	fn run_streaming_output(
		&self,
		program: &str,
		args: &[String],
		file: &Path,
	) -> Result<(), String> {
		let mut child = Command::new(program)
			.args(args)
			.stdout(Stdio::piped())
			.stderr(Stdio::inherit())
			.spawn()
			.map_err(|error| format!("Failed to run {program}: {error}"))?;

		let mut stdout = child
			.stdout
			.take()
			.ok_or_else(|| format!("Failed to capture stdout for {program}"))?;
		let mut output_file = File::create(file)
			.map_err(|error| format!("Failed to write {}: {error}", file.display()))?;
		let mut terminal = std::io::stdout();
		let mut buffer = [0; 8192];

		loop {
			let read = stdout
				.read(&mut buffer)
				.map_err(|error| format!("Failed to read {program} output: {error}"))?;
			if read == 0 {
				break;
			}
			output_file
				.write_all(&buffer[..read])
				.map_err(|error| format!("Failed to write {}: {error}", file.display()))?;
			terminal
				.write_all(&buffer[..read])
				.map_err(|error| error.to_string())?;
			terminal.flush().map_err(|error| error.to_string())?;
		}

		let status = child
			.wait()
			.map_err(|error| format!("Failed to wait for {program}: {error}"))?;
		if status.success() {
			Ok(())
		} else {
			Err(format!("{program} exited with status {status}"))
		}
	}
}

pub(crate) fn require_cmd(program: &str) -> Result<(), String> {
	Command::new(program)
		.arg("--version")
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.status()
		.map(|_| ())
		.map_err(|error| {
			if error.kind() == std::io::ErrorKind::NotFound {
				format!("required command '{program}' was not found")
			} else {
				format!("required command '{program}' could not be executed: {error}")
			}
		})
}

fn resolve_output_dir(workspace_root: &Path, output_dir: &str) -> PathBuf {
	let output_dir = PathBuf::from(output_dir);
	if output_dir.is_absolute() {
		output_dir
	} else {
		workspace_root.join(output_dir)
	}
}

fn slugify(value: &str) -> String {
	value
		.chars()
		.map(|ch| {
			if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
				ch.to_ascii_lowercase()
			} else {
				'_'
			}
		})
		.collect()
}

pub(crate) fn option_or_env_string(value: Option<&str>, env_name: &str, default: &str) -> String {
	value
		.map(ToOwned::to_owned)
		.or_else(|| env::var(env_name).ok())
		.unwrap_or_else(|| default.to_string())
}

fn option_or_env_u16(value: Option<u16>, env_name: &str, default: u16) -> Result<u16, String> {
	if let Some(value) = value {
		return Ok(value);
	}
	env::var(env_name)
		.map(|value| {
			value
				.parse()
				.map_err(|_| format!("Invalid {env_name} value '{value}'"))
		})
		.unwrap_or(Ok(default))
}

pub(crate) fn option_or_env_u64(
	value: Option<u64>,
	env_name: &str,
	default: u64,
) -> Result<u64, String> {
	if let Some(value) = value {
		return Ok(value);
	}
	env::var(env_name)
		.map(|value| {
			value
				.parse()
				.map_err(|_| format!("Invalid {env_name} value '{value}'"))
		})
		.unwrap_or(Ok(default))
}

pub(crate) fn option_or_env_optional_u64(
	value: Option<u64>,
	env_name: &str,
) -> Result<Option<u64>, String> {
	if value.is_some() {
		return Ok(value);
	}
	env::var(env_name)
		.ok()
		.filter(|value| !value.is_empty())
		.map(|value| {
			value
				.parse()
				.map(Some)
				.map_err(|_| format!("Invalid {env_name} value '{value}'"))
		})
		.unwrap_or(Ok(None))
}

fn env_bool(env_name: &str) -> bool {
	matches!(env::var(env_name).as_deref(), Ok("1" | "true"))
}

#[cfg(test)]
mod tests {
	use std::cell::RefCell;
	use std::collections::BTreeSet;
	use std::collections::VecDeque;
	use std::path::Path;
	use std::path::PathBuf;

	use tempfile::tempdir;

	use super::*;

	#[derive(Debug, Clone, PartialEq, Eq)]
	struct RecordedCall {
		program: String,
		args: Vec<String>,
		output_file: Option<PathBuf>,
	}

	#[derive(Debug, Default)]
	struct FakeRunner {
		status_calls: RefCell<Vec<RecordedCall>>,
		streaming_calls: RefCell<Vec<RecordedCall>>,
		output_responses: RefCell<VecDeque<String>>,
		benchmark_output: Option<String>,
	}

	const BENCHMARKED_FULL_PROFILE_COMMANDS: &[&str] = &[
		"APPEND",
		"CLIENT",
		"CONFIG",
		"DECR",
		"DEL",
		"EXISTS",
		"EXPIRE",
		"GET",
		"HELLO",
		"HDEL",
		"HGET",
		"HGETALL",
		"HLEN",
		"HMGET",
		"HSET",
		"INCR",
		"LLEN",
		"LPOP",
		"LPUSH",
		"LRANGE",
		"PING",
		"RPOP",
		"RPUSH",
		"SADD",
		"SCARD",
		"SET",
		"SISMEMBER",
		"SMEMBERS",
		"SREM",
		"TTL",
		"ZADD",
		"ZCARD",
		"ZRANGE",
		"ZREM",
		"ZSCORE",
	];

	impl FakeRunner {
		fn streamed_labels(&self) -> Vec<String> {
			self.streaming_calls
				.borrow()
				.iter()
				.map(|call| {
					let output_path = call
						.output_file
						.as_ref()
						.expect("streaming call has output file");
					output_path
						.file_stem()
						.expect("output file has stem")
						.to_string_lossy()
						.into_owned()
				})
				.collect()
		}

		fn streamed_commands(&self) -> BTreeSet<String> {
			self.streaming_calls
				.borrow()
				.iter()
				.flat_map(|call| benchmarked_commands(&call.args))
				.collect()
		}

		fn status_commands(&self, program: &str) -> Vec<Vec<String>> {
			self.status_calls
				.borrow()
				.iter()
				.filter(|call| call.program == program)
				.map(|call| call.args.clone())
				.collect()
		}
	}

	fn benchmarked_commands(args: &[String]) -> Vec<String> {
		if let Some(index) = args.iter().position(|arg| arg == "-t") {
			return args
				.get(index + 1)
				.into_iter()
				.flat_map(|commands| commands.split(','))
				.map(|command| command.to_ascii_uppercase())
				.collect();
		}

		let known_commands = benchmarked_command_set(BENCHMARKED_FULL_PROFILE_COMMANDS);
		args.iter()
			.find(|arg| known_commands.contains(arg.as_str()))
			.into_iter()
			.cloned()
			.collect()
	}

	fn benchmarked_command_set(commands: &[&str]) -> BTreeSet<String> {
		commands
			.iter()
			.map(|command| (*command).to_string())
			.collect()
	}

	fn args_end_with(args: &[String], suffix: &[&str]) -> bool {
		args.len() >= suffix.len()
			&& args[args.len() - suffix.len()..]
				.iter()
				.map(String::as_str)
				.eq(suffix.iter().copied())
	}

	fn args_contain_pair(args: &[String], option: &str, value: &str) -> bool {
		args.windows(2)
			.any(|window| window[0] == option && window[1] == value)
	}

	impl Runner for FakeRunner {
		fn run_output(&self, program: &str, args: &[String]) -> Result<String, String> {
			self.run_status(program, args)?;
			self.output_responses
				.borrow_mut()
				.pop_front()
				.ok_or_else(|| "missing fake response".into())
		}

		fn run_status(&self, program: &str, args: &[String]) -> Result<(), String> {
			self.status_calls.borrow_mut().push(RecordedCall {
				program: program.to_string(),
				args: args.to_vec(),
				output_file: None,
			});
			Ok(())
		}

		fn run_streaming_output(
			&self,
			program: &str,
			args: &[String],
			file: &Path,
		) -> Result<(), String> {
			self.streaming_calls.borrow_mut().push(RecordedCall {
				program: program.to_string(),
				args: args.to_vec(),
				output_file: Some(file.to_path_buf()),
			});
			fs::write(
				file,
				self.benchmark_output
					.as_deref()
					.unwrap_or("PING_INLINE: 1.00 requests per second\n"),
			)
			.map_err(|error| error.to_string())
		}
	}

	fn test_config(output_dir: PathBuf, profile: Profile) -> Config {
		Config {
			host: "127.0.0.1".into(),
			port: 6379,
			requests: 100,
			clients: 4,
			data_size: 16,
			pipeline: 1,
			random_keyspace: 32,
			threads: Some(2),
			csv: false,
			output_dir,
			seed_requests: 7,
			command: None,
			seed: (profile == Profile::Comparison).then_some(DEFAULT_COMPARISON_SEED),
			settle_millis: 0,
			redis_benchmark: "/bin/echo".into(),
			redis_cli: "/bin/echo".into(),
			extra_args: vec!["--cluster".into()],
			profile,
		}
	}

	#[test]
	fn counted_pop_checks_exact_seed_preflight_and_measured_consumption() {
		for command in ComparisonCommand::value_variants().iter().copied() {
			let Some((direction, count)) = command.counted_pop() else {
				continue;
			};
			for pipeline in [1, 50] {
				let directory = tempdir().unwrap();
				let mut config = test_config(directory.path().into(), Profile::Comparison);
				config.requests = 200;
				config.pipeline = pipeline;
				config.extra_args.clear();
				let runner = FakeRunner {
					output_responses: RefCell::new(VecDeque::from([
						(202 * count).to_string(),
						serde_json::to_string(&vec!["x".repeat(16); count as usize]).unwrap(),
						(201 * count).to_string(),
						count.to_string(),
					])),
					benchmark_output: Some(format!(
						"{direction} bench:list:counted-pop {count}: 1000.00 requests per second, p50=1.0 msec\n"
					)),
					..FakeRunner::default()
				};
				run_counted_pop(&config, &runner, command.label(), direction, count).unwrap();
				let calls = runner.status_calls.borrow();
				assert!(args_contain_pair(&calls[0].args, "-n", "202"));
				assert!(args_contain_pair(&calls[0].args, "-P", "1"));
				assert_eq!(
					calls[0]
						.args
						.iter()
						.filter(|arg| **arg == "x".repeat(16))
						.count(),
					count as usize
				);
				assert!(args_end_with(
					&runner.streaming_calls.borrow()[0].args,
					&[direction, "bench:list:counted-pop", &count.to_string()]
				));
				let validation: serde_json::Value = serde_json::from_slice(
					&fs::read(
						directory
							.path()
							.join(format!("{}-validation.json", command.label())),
					)
					.unwrap(),
				)
				.unwrap();
				assert_eq!(validation["measured_elements"], 200 * count);
				assert_eq!(validation["remaining_elements"], count);
				assert_eq!(validation["elements_per_second"], 1000.0 * count as f64);
			}
		}
	}

	#[test]
	fn counted_pop_rejects_partial_pipeline_or_incorrect_fixture() {
		let directory = tempdir().unwrap();
		let mut config = test_config(directory.path().into(), Profile::Comparison);
		config.extra_args.clear();
		config.pipeline = 50;
		assert!(
			validate_counted_pop_config(&config)
				.unwrap_err()
				.contains("multiple of C * P")
		);
		config.requests = 200;
		for responses in [
			vec!["0".into()],
			vec!["404".into(), "[]".into()],
			vec![
				"404".into(),
				"[\"xxxxxxxxxxxxxxxx\",\"xxxxxxxxxxxxxxxx\"]".into(),
				"402".into(),
				"0".into(),
			],
		] {
			let runner = FakeRunner {
				output_responses: RefCell::new(responses.into()),
				benchmark_output: Some("LPOP: 1000 requests per second\n".into()),
				..FakeRunner::default()
			};
			assert!(run_counted_pop(&config, &runner, "lpop_count_2", "LPOP", 2).is_err());
		}
	}

	#[test]
	fn config_uses_env_style_defaults() {
		let args = Args::default();
		let config = Config::from_args(&args, Path::new("/repo")).unwrap();

		assert_eq!(config.host, "127.0.0.1");
		assert_eq!(config.port, 6379);
		assert_eq!(config.requests, 500000);
		assert_eq!(config.clients, 50);
		assert_eq!(config.data_size, 128);
		assert_eq!(config.pipeline, 1);
		assert_eq!(config.random_keyspace, 100000);
		assert_eq!(config.command, None);
		assert_eq!(config.seed, None);
		assert_eq!(config.settle_millis, 0);
		assert_eq!(config.output_dir, Path::new("/repo/target/redis-benchmark"));
		assert_eq!(config.output_ext(), "txt");
	}

	#[test]
	fn csv_config_uses_csv_extension() {
		let args = Args {
			csv: true,
			..Args::default()
		};
		let config = Config::from_args(&args, Path::new("/repo")).unwrap();

		assert_eq!(config.output_ext(), "csv");
		assert!(config.benchmark_base_args().contains(&"--csv".to_string()));
		assert!(!config.benchmark_base_args().contains(&"-q".to_string()));
	}

	#[test]
	fn force_quiet_overrides_csv_for_report_callers() {
		let args = Args {
			csv: true,
			force_quiet: true,
			..Args::default()
		};
		let config = Config::from_args(&args, Path::new("/repo")).unwrap();

		assert_eq!(config.output_ext(), "txt");
		assert!(config.benchmark_base_args().contains(&"-q".to_string()));
		assert!(!config.benchmark_base_args().contains(&"--csv".to_string()));
	}

	#[test]
	fn benchmark_base_args_include_threads_when_requested() {
		let args = Args {
			threads: Some(4),
			..Args::default()
		};
		let config = Config::from_args(&args, Path::new("/repo")).unwrap();

		assert_eq!(
			config.benchmark_base_args(),
			vec![
				"-h",
				"127.0.0.1",
				"-p",
				"6379",
				"-n",
				"500000",
				"-c",
				"50",
				"-d",
				"128",
				"-r",
				"100000",
				"-P",
				"1",
				"--threads",
				"4",
				"-q",
			]
		);
	}

	#[test]
	fn output_dir_allows_absolute_paths() {
		let args = Args {
			output_dir: Some("/tmp/nimbis-bench".into()),
			..Args::default()
		};
		let config = Config::from_args(&args, Path::new("/repo")).unwrap();

		assert_eq!(config.output_dir, Path::new("/tmp/nimbis-bench"));
	}

	#[test]
	fn comparison_profile_is_configurable() {
		let args = Args {
			profile: Profile::Comparison,
			..Args::default()
		};
		let config = Config::from_args(&args, Path::new("/repo")).unwrap();

		assert_eq!(config.profile, Profile::Comparison);
		assert_eq!(config.seed, Some(DEFAULT_COMPARISON_SEED));
	}

	#[test]
	fn full_profile_rejects_command_filter() {
		let args = Args {
			command: Some(ComparisonCommand::Get),
			..Args::default()
		};

		let error = Config::from_args(&args, Path::new("/repo")).unwrap_err();

		assert_eq!(error, "--command requires --profile comparison");
	}

	#[test]
	fn config_keeps_seed_and_settle_millis() {
		let args = Args {
			profile: Profile::Comparison,
			command: Some(ComparisonCommand::Srem),
			seed: Some(42),
			settle_millis: Some(1_000),
			..Args::default()
		};

		let config = Config::from_args(&args, Path::new("/repo")).unwrap();

		assert_eq!(config.command, Some(ComparisonCommand::Srem));
		assert_eq!(config.seed, Some(42));
		assert_eq!(config.settle_millis, 1_000);
		assert_eq!(ComparisonCommand::Srem.as_str(), "SREM");
	}

	#[test]
	fn config_rejects_zero_seed_requests() {
		let args = Args {
			seed_requests: Some(0),
			..Args::default()
		};

		let error = Config::from_args(&args, Path::new("/repo")).unwrap_err();

		assert_eq!(error, "--seed-n must be greater than zero");
	}

	#[test]
	fn config_rejects_seed_above_redis_8_integer_range() {
		let args = Args {
			seed: Some(MAX_REDIS_RANDOM_SEED + 1),
			..Args::default()
		};

		let error = Config::from_args(&args, Path::new("/repo")).unwrap_err();

		assert!(error.contains(&MAX_REDIS_RANDOM_SEED.to_string()));
	}

	#[test]
	fn comparison_command_fixtures_are_isolated() {
		let tempdir = tempdir().unwrap();
		let mut config = test_config(tempdir.path().join("fixtures"), Profile::Comparison);
		config.seed = Some(42);

		let cases: &[(ComparisonCommand, &[&str])] = &[
			(ComparisonCommand::Get, &["-t", "set"]),
			(
				ComparisonCommand::Hget,
				&["HSET", "bench:hash", "field1", "xxxxxxxxxxxxxxxx"],
			),
			(ComparisonCommand::Lpop, &["-t", "lpush"]),
			(
				ComparisonCommand::Srem,
				&["SADD", "bench:set:srem", "xxxx__rand_int__"],
			),
			(
				ComparisonCommand::Zrem,
				&["ZADD", "bench:zset:zrem", "1", "xxxx__rand_int__"],
			),
		];
		for (command, expected_suffix) in cases {
			let runner = FakeRunner::default();

			seed_comparison_command(&config, &runner, *command).unwrap();

			let calls = runner.status_calls.borrow();
			assert_eq!(calls.len(), 1, "fixture for {}", command.as_str());
			assert!(args_end_with(&calls[0].args, expected_suffix));
			if *command != ComparisonCommand::Hget {
				assert!(args_contain_pair(&calls[0].args, "--seed", "42"));
			}
		}

		for command in [
			ComparisonCommand::Set,
			ComparisonCommand::Hset,
			ComparisonCommand::Lpush,
			ComparisonCommand::Sadd,
			ComparisonCommand::Zadd,
		] {
			let runner = FakeRunner::default();

			seed_comparison_command(&config, &runner, command).unwrap();

			assert!(
				runner.status_calls.borrow().is_empty(),
				"write fixture for {} must stay empty",
				command.as_str()
			);
		}
	}

	#[test]
	fn comparison_command_measurements_are_filtered() {
		let tempdir = tempdir().unwrap();
		let config = test_config(tempdir.path().to_path_buf(), Profile::Comparison);
		for command in [
			ComparisonCommand::Get,
			ComparisonCommand::Set,
			ComparisonCommand::Hget,
			ComparisonCommand::Hset,
			ComparisonCommand::Lpush,
			ComparisonCommand::Lpop,
			ComparisonCommand::Sadd,
			ComparisonCommand::Srem,
			ComparisonCommand::Zadd,
			ComparisonCommand::Zrem,
		] {
			let runner = FakeRunner::default();

			run_comparison_command(&config, &runner, command).unwrap();

			assert_eq!(runner.streamed_labels(), vec![command.label()]);
			assert_eq!(
				runner.streamed_commands(),
				benchmarked_command_set(&[command.as_str()])
			);
			if matches!(
				command,
				ComparisonCommand::Sadd
					| ComparisonCommand::Srem
					| ComparisonCommand::Zadd
					| ComparisonCommand::Zrem
			) {
				let calls = runner.streaming_calls.borrow();
				let member = calls[0].args.last().unwrap();
				assert_eq!(member.len(), config.data_size as usize);
				assert!(member.ends_with(RANDOM_TOKEN));
			}
		}
	}

	#[test]
	fn comparison_payloads_match_the_configured_data_size() {
		assert_eq!(fixed_payload(16).unwrap(), "xxxxxxxxxxxxxxxx");
		assert_eq!(random_payload(16).unwrap(), "xxxx__rand_int__");
		assert!(random_payload(11).is_err());
	}

	#[test]
	fn redis_cli_setup_fails_on_server_errors() {
		let tempdir = tempdir().unwrap();
		let config = test_config(tempdir.path().to_path_buf(), Profile::Comparison);
		let runner = FakeRunner::default();

		redis_cli(&config, &runner, &["PING"]).unwrap();

		let calls = runner.status_calls.borrow();
		assert_eq!(calls.len(), 1);
		assert!(calls[0].args.iter().any(|arg| arg == "-e"));
		assert!(args_end_with(&calls[0].args, &["PING"]));
	}

	#[test]
	fn selected_comparison_command_only_runs_that_command_with_seed() {
		let tempdir = tempdir().unwrap();
		let mut config = test_config(tempdir.path().join("redis-benchmark"), Profile::Comparison);
		config.command = Some(ComparisonCommand::Get);
		config.seed = Some(42);
		let runner = FakeRunner::default();

		run_with_runner(&config, &runner).unwrap();

		assert_eq!(runner.streamed_labels(), vec!["get"]);
		assert_eq!(
			runner.streamed_commands(),
			benchmarked_command_set(&["GET"])
		);
		let streaming_calls = runner.streaming_calls.borrow();
		assert_eq!(streaming_calls.len(), 1);
		assert!(args_contain_pair(&streaming_calls[0].args, "--seed", "42"));

		let setup_calls = runner.status_calls.borrow();
		let setup = setup_calls
			.iter()
			.find(|call| args_end_with(&call.args, &["-t", "set"]))
			.expect("GET fixture should be seeded with SET");
		assert!(args_contain_pair(&setup.args, "--seed", "42"));
		assert!(!setup_calls.iter().any(|call| {
			call.args
				.iter()
				.any(|arg| arg == "bench:string:a:__rand_int__")
		}));
		assert!(config.output_dir.join("get.txt").exists());
		assert!(!config.output_dir.join("builtin_comparison.txt").exists());
	}

	#[test]
	fn run_with_runner_executes_full_profile_suites_and_writes_outputs() {
		let tempdir = tempdir().unwrap();
		let config = test_config(tempdir.path().join("redis-benchmark"), Profile::Full);
		let runner = FakeRunner::default();

		run_with_runner(&config, &runner).unwrap();

		let labels = runner.streamed_labels();
		assert!(labels.contains(&"builtin_supported".to_string()));
		assert!(labels.contains(&"del_multi_key".to_string()));
		assert!(!labels.contains(&"lrange".to_string()));
		assert!(labels.contains(&"hello_2".to_string()));
		assert!(labels.contains(&"client_id".to_string()));
		assert_eq!(labels.len(), 24);
		assert_eq!(
			runner.streamed_commands(),
			benchmarked_command_set(BENCHMARKED_FULL_PROFILE_COMMANDS)
		);

		let redis_cli_calls = runner.status_commands("/bin/echo");
		assert!(
			redis_cli_calls
				.iter()
				.any(|args| args.ends_with(&["PING".into()]))
		);
		assert!(
			redis_cli_calls
				.iter()
				.any(|args| args.ends_with(&["FLUSHDB".into()]))
		);
		assert!(redis_cli_calls.iter().any(|args| args.ends_with(&[
			"SET".into(),
			"bench:string:get".into(),
			"value".into()
		])));
		assert!(redis_cli_calls.iter().any(|args| {
			args.windows(4).any(|window| {
				window
					== [
						"HSET".to_string(),
						"bench:hash".to_string(),
						"field1".to_string(),
						"value1".to_string(),
					]
			})
		}));
		assert!(
			redis_cli_calls
				.iter()
				.any(|args| args_end_with(args, &["DEL", "LIST", "bench:list"]))
		);
		assert!(
			redis_cli_calls
				.iter()
				.any(|args| args_end_with(args, &["DEL", "SET", "bench:set:a", "bench:set:b"]))
		);
		assert!(
			redis_cli_calls
				.iter()
				.any(|args| args_end_with(args, &["DEL", "ZSET", "bench:zset"]))
		);

		let streamed_calls = runner.streaming_calls.borrow();
		assert!(streamed_calls.iter().any(|call| {
			call.args.windows(2).any(|window| {
				window[0] == "-t" && window[1].split(',').any(|test| test == "lrange")
			})
		}));
		assert!(streamed_calls.iter().any(|call| args_end_with(
			&call.args,
			&[
				"DEL",
				"STRING",
				"bench:string:del:a:__rand_int__",
				"bench:string:del:b:__rand_int__",
			]
		)));
		assert!(streamed_calls.iter().any(|call| args_end_with(
			&call.args,
			&[
				"EXISTS",
				"STRING",
				"bench:string:a:__rand_int__",
				"bench:string:b:__rand_int__",
				"bench:string:missing:__rand_int__",
			]
		)));
		assert!(streamed_calls.iter().any(|call| args_end_with(
			&call.args,
			&[
				"EXPIRE",
				"STRING",
				"bench:string:expire:__rand_int__",
				"300",
			]
		)));
		assert!(
			streamed_calls
				.iter()
				.any(|call| args_end_with(&call.args, &["TTL", "STRING", "bench:string:ttl"]))
		);
		assert!(config.output_dir.join("builtin_supported.txt").exists());
		assert!(!config.output_dir.join("lrange.txt").exists());
		assert!(config.output_dir.join("client_id.txt").exists());
	}

	#[test]
	fn run_with_runner_executes_comparison_profile_only() {
		let tempdir = tempdir().unwrap();
		let config = test_config(tempdir.path().join("redis-benchmark"), Profile::Comparison);
		let runner = FakeRunner::default();

		run_with_runner(&config, &runner).unwrap();

		let labels = runner.streamed_labels();
		assert_eq!(
			labels,
			vec![
				"builtin_comparison".to_string(),
				"hget".to_string(),
				"srem".to_string(),
				"zrem".to_string(),
			]
		);
		assert_eq!(
			runner.streamed_commands(),
			benchmarked_command_set(COMPARISON_PROFILE_COMMANDS)
		);
		assert!(runner.streaming_calls.borrow().iter().all(|call| {
			args_contain_pair(&call.args, "--seed", "279000")
				&& call.args.windows(2).all(|window| {
					window[0] != "-t" || !window[1].split(',').any(|test| test == "lrange")
				})
		}));
		assert!(
			runner
				.status_calls
				.borrow()
				.iter()
				.filter(|call| call.args.iter().any(|arg| arg == "-n"))
				.all(|call| args_contain_pair(&call.args, "--seed", "279000"))
		);
		let setup_calls = runner.status_commands("/bin/echo");
		assert!(
			setup_calls
				.iter()
				.any(|args| args_end_with(args, &["DEL", "LIST", "bench:list"]))
		);
		assert!(!config.output_dir.join("hello_2.txt").exists());
	}
}
