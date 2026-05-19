use std::env;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use clap::Args as ClapArgs;
use clap::ValueEnum;

use crate::write_stdout_line;

const BUILTIN_SUPPORTED: &str = "ping,set,get,incr,lpush,rpush,lpop,rpop,sadd,hset,zadd";

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

	/// Result directory.
	#[arg(long)]
	pub output_dir: Option<String>,

	/// Setup request count for seeded random data.
	#[arg(long = "seed-n")]
	pub seed_requests: Option<u64>,

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

#[derive(Debug)]
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

		Ok(Self {
			host: option_or_env_string(args.host.as_deref(), "HOST", "127.0.0.1"),
			port: option_or_env_u16(args.port, "PORT", 6379)?,
			requests,
			clients: option_or_env_u64(args.clients, "C", 50)?,
			data_size: option_or_env_u64(args.data_size, "D", 128)?,
			pipeline: option_or_env_u64(args.pipeline, "P", 1)?,
			random_keyspace: option_or_env_u64(args.random_keyspace, "R", 100000)?,
			threads: option_or_env_optional_u64(args.threads, "THREADS")?,
			csv: args.csv || env_bool("CSV"),
			output_dir,
			seed_requests: option_or_env_u64(args.seed_requests, "SEED_N", requests)?,
			redis_benchmark: option_or_env_string(
				args.redis_benchmark.as_deref(),
				"REDIS_BENCHMARK",
				"redis-benchmark",
			),
			redis_cli: option_or_env_string(args.redis_cli.as_deref(), "REDIS_CLI", "redis-cli"),
			extra_args: args.extra_args.clone(),
			profile: args.profile,
		})
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
	seed_fixed_data(config, runner)?;
	seed_random_data(config, runner)?;
	match config.profile {
		Profile::Full => {
			run_builtin_suite(config, runner)?;
			run_custom_suite(config, runner)?;
			run_control_smoke_suite(config, runner)?;
		}
		Profile::Comparison => run_comparison_suite(config, runner)?,
	}

	write_stdout_line("")?;
	write_stdout_line(&format!(
		"redis-benchmark results written to {}",
		config.output_dir.display()
	))?;
	Ok(())
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
	redis_cli(config, runner, &["DEL", "bench:list"])?;
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
	redis_cli(config, runner, &["DEL", "bench:set:a", "bench:set:b"])?;
	redis_cli(config, runner, &["SADD", "bench:set:a", "a", "b", "c"])?;
	redis_cli(config, runner, &["SADD", "bench:set:b", "b", "c", "d"])?;
	redis_cli(config, runner, &["DEL", "bench:zset"])?;
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
				"bench:string:del:a:__rand_int__",
				"bench:string:del:b:__rand_int__",
			],
		),
		(
			"exists_multi_key",
			&[
				"EXISTS",
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
		("lrange", &["LRANGE", "bench:list", "0", "-1"]),
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
			&["EXPIRE", "bench:string:expire:__rand_int__", "300"],
		),
		("ttl", &["TTL", "bench:string:ttl"]),
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
	];
	args.extend(command_args.iter().map(|arg| (*arg).to_string()));
	runner.run_status(&config.redis_cli, &args)
}

struct ProcessRunner;

impl Runner for ProcessRunner {
	fn run_status(&self, program: &str, args: &[String]) -> Result<(), String> {
		let output = Command::new(program)
			.args(args)
			.output()
			.map_err(|error| format!("Failed to run {program}: {error}"))?;
		if output.status.success() {
			Ok(())
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

fn require_cmd(program: &str) -> Result<(), String> {
	if program.contains(std::path::MAIN_SEPARATOR) {
		if Path::new(program).exists() {
			return Ok(());
		}
		return Err(format!("required command '{program}' was not found"));
	}

	for path in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
		let candidate = path.join(program);
		if candidate.exists() {
			return Ok(());
		}
	}
	Err(format!(
		"required command '{program}' was not found in PATH"
	))
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

fn option_or_env_string(value: Option<&str>, env_name: &str, default: &str) -> String {
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

fn option_or_env_u64(value: Option<u64>, env_name: &str, default: u64) -> Result<u64, String> {
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

fn option_or_env_optional_u64(value: Option<u64>, env_name: &str) -> Result<Option<u64>, String> {
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

	const BENCHMARKED_COMPARISON_PROFILE_COMMANDS: &[&str] = &[
		"GET", "HGET", "HSET", "LPOP", "LPUSH", "SADD", "SET", "SREM", "ZADD", "ZREM",
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

	impl Runner for FakeRunner {
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
			fs::write(file, b"PING_INLINE: 1.00 requests per second\n")
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
			redis_benchmark: "/bin/echo".into(),
			redis_cli: "/bin/echo".into(),
			extra_args: vec!["--cluster".into()],
			profile,
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
		assert!(labels.contains(&"lrange".to_string()));
		assert!(labels.contains(&"hello_2".to_string()));
		assert!(labels.contains(&"client_id".to_string()));
		assert_eq!(labels.len(), 25);
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
		assert!(config.output_dir.join("builtin_supported.txt").exists());
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
			benchmarked_command_set(BENCHMARKED_COMPARISON_PROFILE_COMMANDS)
		);
		assert!(!config.output_dir.join("hello_2.txt").exists());
	}
}
