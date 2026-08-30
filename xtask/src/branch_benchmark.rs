use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::fs::TryLockError;
use std::net::TcpListener;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use clap::Args as ClapArgs;
use tempfile::Builder;

use crate::benchmarks;
use crate::redis_benchmark;
use crate::write_stdout;
use crate::write_stdout_line;

const DEFAULT_REQUESTS: u64 = 200_000;
const DEFAULT_CLIENTS: u64 = 100;
const DEFAULT_DATA_SIZE: u64 = 512;
const DEFAULT_RANDOM_KEYSPACE: u64 = redis_benchmark::MAX_REDIS_RANDOM_SEED;
const DEFAULT_PIPELINE_DEPTH: u64 = 50;
const DEFAULT_STARTUP_TIMEOUT_SECONDS: u64 = 15;

#[derive(ClapArgs, Debug)]
pub struct Args {
	/// Git ref used as the comparison base.
	#[arg(long, default_value = "main")]
	pub base: String,

	/// Git ref being evaluated.
	#[arg(long, default_value = "HEAD")]
	pub head: String,

	/// Request count per benchmark. Defaults to N or 200000.
	#[arg(long = "n")]
	pub requests: Option<u64>,

	/// Concurrent clients. Defaults to C or 100.
	#[arg(long = "c")]
	pub clients: Option<u64>,

	/// Payload size. Defaults to D or 512.
	#[arg(long = "d")]
	pub data_size: Option<u64>,

	/// Random key space. Defaults to R or 2147483647.
	#[arg(long = "r")]
	pub random_keyspace: Option<u64>,

	/// Pipeline depth used for the pipelined comparison.
	#[arg(long, default_value_t = DEFAULT_PIPELINE_DEPTH)]
	pub pipeline_depth: u64,

	/// Optional redis-benchmark --threads value. Defaults to THREADS when set.
	#[arg(long)]
	pub threads: Option<u64>,

	/// Setup request count for seeded data. Defaults to SEED_N or N.
	#[arg(long = "seed-n")]
	pub seed_requests: Option<u64>,

	/// Fixed server port. By default an available loopback port is selected.
	#[arg(long)]
	pub port: Option<u16>,

	/// Number of Tokio worker threads used by both servers.
	#[arg(long)]
	pub runtime_threads: Option<usize>,

	/// Seconds to wait for each server to answer PING.
	#[arg(long, default_value_t = DEFAULT_STARTUP_TIMEOUT_SECONDS)]
	pub startup_timeout_seconds: u64,

	/// Parent directory for run artifacts. Defaults to
	/// target/redis-benchmark-compare.
	#[arg(long)]
	pub output_dir: Option<PathBuf>,

	/// Override redis-benchmark binary name/path.
	#[arg(long)]
	pub redis_benchmark: Option<String>,

	/// Override redis-cli binary name/path.
	#[arg(long)]
	pub redis_cli: Option<String>,
}

impl Default for Args {
	fn default() -> Self {
		Self {
			base: "main".into(),
			head: "HEAD".into(),
			requests: None,
			clients: None,
			data_size: None,
			random_keyspace: None,
			pipeline_depth: DEFAULT_PIPELINE_DEPTH,
			threads: None,
			seed_requests: None,
			port: None,
			runtime_threads: None,
			startup_timeout_seconds: DEFAULT_STARTUP_TIMEOUT_SECONDS,
			output_dir: None,
			redis_benchmark: None,
			redis_cli: None,
		}
	}
}

#[derive(Debug)]
struct Config {
	base_ref: String,
	head_ref: String,
	requests: u64,
	clients: u64,
	data_size: u64,
	random_keyspace: u64,
	pipeline_depth: u64,
	threads: Option<u64>,
	seed_requests: u64,
	port: Option<u16>,
	runtime_threads: Option<usize>,
	startup_timeout: Duration,
	output_parent: PathBuf,
	redis_benchmark: String,
	redis_cli: String,
}

impl Config {
	fn from_args(args: Args, workspace_root: &Path) -> Result<Self, String> {
		let requests = redis_benchmark::option_or_env_u64(args.requests, "N", DEFAULT_REQUESTS)?;
		let clients = redis_benchmark::option_or_env_u64(args.clients, "C", DEFAULT_CLIENTS)?;
		let data_size = redis_benchmark::option_or_env_u64(args.data_size, "D", DEFAULT_DATA_SIZE)?;
		let random_keyspace =
			redis_benchmark::option_or_env_u64(args.random_keyspace, "R", DEFAULT_RANDOM_KEYSPACE)?;
		let threads = redis_benchmark::option_or_env_optional_u64(args.threads, "THREADS")?;
		let seed_requests =
			redis_benchmark::option_or_env_u64(args.seed_requests, "SEED_N", requests)?;
		let output_parent = args
			.output_dir
			.or_else(|| env::var_os("COMPARE_OUTPUT_DIR").map(PathBuf::from))
			.unwrap_or_else(|| workspace_root.join("target/redis-benchmark-compare"));
		let output_parent = if output_parent.is_absolute() {
			output_parent
		} else {
			workspace_root.join(output_parent)
		};

		let config = Self {
			base_ref: args.base,
			head_ref: args.head,
			requests,
			clients,
			data_size,
			random_keyspace,
			pipeline_depth: args.pipeline_depth,
			threads,
			seed_requests,
			port: args.port,
			runtime_threads: args.runtime_threads,
			startup_timeout: Duration::from_secs(args.startup_timeout_seconds),
			output_parent,
			redis_benchmark: redis_benchmark::option_or_env_string(
				args.redis_benchmark.as_deref(),
				"REDIS_BENCHMARK",
				"redis-benchmark",
			),
			redis_cli: redis_benchmark::option_or_env_string(
				args.redis_cli.as_deref(),
				"REDIS_CLI",
				"redis-cli",
			),
		};
		config.validate()?;
		Ok(config)
	}

	fn validate(&self) -> Result<(), String> {
		for (name, value) in [
			("N", self.requests),
			("C", self.clients),
			("D", self.data_size),
			("R", self.random_keyspace),
			("pipeline depth", self.pipeline_depth),
			("SEED_N", self.seed_requests),
		] {
			if value == 0 {
				return Err(format!("{name} must be greater than zero"));
			}
		}
		if self.startup_timeout.is_zero() {
			return Err("startup timeout must be greater than zero".into());
		}
		if self.port == Some(0) {
			return Err("port must be greater than zero".into());
		}
		if self.runtime_threads == Some(0) {
			return Err("runtime threads must be greater than zero".into());
		}
		if self.base_ref.trim().is_empty() || self.head_ref.trim().is_empty() {
			return Err("base and head Git refs must not be empty".into());
		}
		Ok(())
	}
}

#[derive(Debug)]
struct BenchmarkTarget {
	ref_name: String,
	commit: String,
	binary: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct Cancellation {
	requested: Arc<AtomicBool>,
}

impl Cancellation {
	pub(crate) fn install() -> Result<Self, String> {
		let cancellation = Self::new();
		let signal_cancellation = cancellation.clone();
		ctrlc::set_handler(move || signal_cancellation.request())
			.map_err(|error| format!("Failed to install Ctrl-C handler: {error}"))?;
		Ok(cancellation)
	}

	fn new() -> Self {
		Self {
			requested: Arc::new(AtomicBool::new(false)),
		}
	}

	fn request(&self) {
		self.requested.store(true, Ordering::SeqCst);
	}

	pub(crate) fn check(&self) -> Result<(), String> {
		if self.requested.load(Ordering::SeqCst) {
			Err("Benchmark comparison interrupted".into())
		} else {
			Ok(())
		}
	}
}

pub fn run(args: Args, workspace_root: &Path) -> Result<(), String> {
	let cancellation = Cancellation::install()?;
	run_with_cancellation(args, workspace_root, &cancellation)
}

fn run_with_cancellation(
	args: Args,
	workspace_root: &Path,
	cancellation: &Cancellation,
) -> Result<(), String> {
	cancellation.check()?;
	let config = Config::from_args(args, workspace_root)?;
	let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());

	redis_benchmark::require_cmd("git")?;
	redis_benchmark::require_cmd(&cargo)?;
	redis_benchmark::require_cmd(&config.redis_benchmark)?;
	redis_benchmark::require_cmd(&config.redis_cli)?;
	cancellation.check()?;

	let base_commit = resolve_commit(workspace_root, &config.base_ref)?;
	cancellation.check()?;
	let head_commit = resolve_commit(workspace_root, &config.head_ref)?;
	warn_if_worktree_is_dirty(workspace_root)?;
	cancellation.check()?;
	if base_commit == head_commit {
		return Err(format!(
			"base '{}' and head '{}' resolve to the same commit {}",
			config.base_ref,
			config.head_ref,
			short_commit(&base_commit)
		));
	}

	let run_dir = create_run_dir(&config.output_parent)?;
	let temporary = Builder::new()
		.prefix("nimbis-redis-benchmark-")
		.tempdir()
		.map_err(|error| format!("Failed to create temporary benchmark directory: {error}"))?;
	let clone_dir = temporary.path().join("repo");
	let target_dir = config.output_parent.join("build-cache");

	write_stdout_line("Nimbis Redis benchmark branch comparison")?;
	write_stdout_line(&format!(
		"base={} ({}) head={} ({})",
		config.base_ref,
		short_commit(&base_commit),
		config.head_ref,
		short_commit(&head_commit)
	))?;
	write_stdout_line(&format!(
		"n={} clients={} data_size={} random_keyspace={} pipelines=1,{} redis_threads={} runtime_threads={}",
		config.requests,
		config.clients,
		config.data_size,
		config.random_keyspace,
		config.pipeline_depth,
		optional_value(config.threads, "default"),
		optional_value(config.runtime_threads, "auto")
	))?;
	write_stdout_line("Benchmarks run sequentially with isolated file stores.")?;
	write_stdout_line(&format!("Artifacts: {}", run_dir.display()))?;
	write_stdout_line("")?;

	clone_repository(workspace_root, &clone_dir, cancellation)?;
	let base_binary = build_binary(
		&cargo,
		&clone_dir,
		&target_dir,
		&base_commit,
		"base",
		temporary.path(),
		cancellation,
	)?;
	let head_binary = build_binary(
		&cargo,
		&clone_dir,
		&target_dir,
		&head_commit,
		"head",
		temporary.path(),
		cancellation,
	)?;

	let base = BenchmarkTarget {
		ref_name: config.base_ref.clone(),
		commit: base_commit,
		binary: base_binary,
	};
	let head = BenchmarkTarget {
		ref_name: config.head_ref.clone(),
		commit: head_commit,
		binary: head_binary,
	};

	let base_output = run_benchmark_pass(
		&config,
		&base,
		"base-p1",
		1,
		&run_dir,
		temporary.path(),
		cancellation,
	)?;
	let head_output = run_benchmark_pass(
		&config,
		&head,
		"head-p1",
		1,
		&run_dir,
		temporary.path(),
		cancellation,
	)?;
	let base_pipeline_output = run_benchmark_pass(
		&config,
		&base,
		"base-pipeline",
		config.pipeline_depth,
		&run_dir,
		temporary.path(),
		cancellation,
	)?;
	let head_pipeline_output = run_benchmark_pass(
		&config,
		&head,
		"head-pipeline",
		config.pipeline_depth,
		&run_dir,
		temporary.path(),
		cancellation,
	)?;

	cancellation.check()?;
	let comparison = benchmarks::build_report(&benchmarks::Args {
		main: base_output.display().to_string(),
		pr: head_output.display().to_string(),
		baselines: Vec::new(),
		main_pipeline: base_pipeline_output.display().to_string(),
		pr_pipeline: head_pipeline_output.display().to_string(),
		baseline_pipelines: Vec::new(),
		main_label: format!("{} ({})", base.ref_name, short_commit(&base.commit)),
		pr_label: format!("{} ({})", head.ref_name, short_commit(&head.commit)),
		pipeline_depth: config.pipeline_depth,
	})?;
	let report = build_full_report(&config, &base, &head, &comparison);
	let report_path = run_dir.join("report.md");
	fs::write(&report_path, &report)
		.map_err(|error| format!("Failed to write {}: {error}", report_path.display()))?;

	write_stdout("\n")?;
	write_stdout(&report)?;
	write_stdout_line(&format!("\nArtifacts: {}", run_dir.display()))?;
	cancellation.check()?;
	Ok(())
}

fn resolve_commit(workspace_root: &Path, git_ref: &str) -> Result<String, String> {
	let revision = format!("{git_ref}^{{commit}}");
	let output = Command::new("git")
		.current_dir(workspace_root)
		.args(["rev-parse", "--verify", "--end-of-options", &revision])
		.output()
		.map_err(|error| format!("Failed to resolve Git ref '{git_ref}': {error}"))?;
	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
		return Err(format!("Failed to resolve Git ref '{git_ref}': {stderr}"));
	}
	String::from_utf8(output.stdout)
		.map(|value| value.trim().to_string())
		.map_err(|error| format!("Git returned an invalid commit for '{git_ref}': {error}"))
}

fn clone_repository(
	workspace_root: &Path,
	clone_dir: &Path,
	cancellation: &Cancellation,
) -> Result<(), String> {
	write_stdout_line("Cloning the local repository into an isolated temporary workspace...")?;
	run_checked(
		Command::new("git")
			.args([
				"clone",
				"--quiet",
				"--no-checkout",
				"--local",
				"--no-hardlinks",
			])
			.arg(workspace_root)
			.arg(clone_dir),
		"clone the local repository",
		cancellation,
	)
}

fn build_binary(
	cargo: &str,
	clone_dir: &Path,
	target_dir: &Path,
	commit: &str,
	label: &str,
	temporary_root: &Path,
	cancellation: &Cancellation,
) -> Result<PathBuf, String> {
	write_stdout_line(&format!(
		"Building {label} commit {} in release mode...",
		short_commit(commit)
	))?;
	run_checked(
		Command::new("git")
			.current_dir(clone_dir)
			.args(["checkout", "--quiet", "--detach", commit]),
		&format!("check out {label} commit"),
		cancellation,
	)?;
	let build_lock = lock_build_cache(target_dir, cancellation)?;
	run_checked(
		Command::new(cargo)
			.current_dir(clone_dir)
			.args(["build", "--release", "--package", "nimbis", "--target-dir"])
			.arg(target_dir),
		&format!("build {label} commit"),
		cancellation,
	)?;

	let executable = format!("nimbis{}", env::consts::EXE_SUFFIX);
	let built_binary = target_dir.join("release").join(&executable);
	if !built_binary.is_file() {
		return Err(format!(
			"built {label} binary was not found at {}",
			built_binary.display()
		));
	}
	let binary_dir = temporary_root.join("binaries").join(label);
	fs::create_dir_all(&binary_dir)
		.map_err(|error| format!("Failed to create {}: {error}", binary_dir.display()))?;
	let binary = binary_dir.join(executable);
	fs::copy(&built_binary, &binary).map_err(|error| {
		format!(
			"Failed to copy {} to {}: {error}",
			built_binary.display(),
			binary.display()
		)
	})?;
	cancellation.check()?;
	drop(build_lock);
	Ok(binary)
}

fn lock_build_cache(target_dir: &Path, cancellation: &Cancellation) -> Result<File, String> {
	fs::create_dir_all(target_dir)
		.map_err(|error| format!("Failed to create {}: {error}", target_dir.display()))?;
	let lock_path = target_dir.join("branch-compare.lock");
	let lock = OpenOptions::new()
		.create(true)
		.read(true)
		.write(true)
		.truncate(false)
		.open(&lock_path)
		.map_err(|error| format!("Failed to open {}: {error}", lock_path.display()))?;
	loop {
		cancellation.check()?;
		match lock.try_lock() {
			Ok(()) => {
				cancellation.check()?;
				return Ok(lock);
			}
			Err(TryLockError::WouldBlock) => {
				thread::sleep(Duration::from_millis(100));
			}
			Err(TryLockError::Error(error)) => {
				return Err(format!("Failed to lock {}: {error}", lock_path.display()));
			}
		}
	}
}

fn run_benchmark_pass(
	config: &Config,
	target: &BenchmarkTarget,
	slot: &str,
	pipeline: u64,
	run_dir: &Path,
	temporary_root: &Path,
	cancellation: &Cancellation,
) -> Result<PathBuf, String> {
	cancellation.check()?;
	write_stdout_line(&format!(
		"\n==> {} ({}) with pipeline depth {}",
		target.ref_name,
		short_commit(&target.commit),
		pipeline
	))?;
	let port = match config.port {
		Some(port) => port,
		None => pick_available_port()?,
	};
	ensure_port_available(port)?;

	let artifact_dir = run_dir.join(slot);
	let runtime_dir = temporary_root.join("runtime").join(slot);
	let suites_dir = artifact_dir.join("suites");
	fs::create_dir_all(&suites_dir)
		.map_err(|error| format!("Failed to create {}: {error}", suites_dir.display()))?;
	fs::create_dir_all(&runtime_dir)
		.map_err(|error| format!("Failed to create {}: {error}", runtime_dir.display()))?;

	let log_path = artifact_dir.join("server.log");
	let mut server = ServerProcess::start(
		&target.binary,
		&runtime_dir,
		&log_path,
		port,
		config.runtime_threads,
	)?;
	server.wait_until_ready(
		&config.redis_cli,
		"127.0.0.1",
		port,
		config.startup_timeout,
		cancellation,
	)?;
	cancellation.check()?;

	let benchmark_result = redis_benchmark::run(
		redis_benchmark::Args {
			host: Some("127.0.0.1".into()),
			port: Some(port),
			requests: Some(config.requests),
			clients: Some(config.clients),
			data_size: Some(config.data_size),
			pipeline: Some(pipeline),
			random_keyspace: Some(config.random_keyspace),
			threads: config.threads,
			csv: false,
			force_quiet: true,
			output_dir: Some(suites_dir.display().to_string()),
			seed_requests: Some(config.seed_requests),
			command: None,
			seed: Some(redis_benchmark::DEFAULT_COMPARISON_SEED),
			settle_millis: None,
			redis_benchmark: Some(config.redis_benchmark.clone()),
			redis_cli: Some(config.redis_cli.clone()),
			extra_args: Vec::new(),
			profile: redis_benchmark::Profile::Comparison,
		},
		run_dir,
	);
	let stop_result = server.stop();
	finish_benchmark_pass(
		cancellation,
		&target.ref_name,
		pipeline,
		benchmark_result,
		stop_result,
	)?;

	let combined_output = run_dir.join(format!("{slot}.txt"));
	combine_suite_outputs(&suites_dir, &combined_output)?;
	Ok(combined_output)
}

fn finish_benchmark_pass(
	cancellation: &Cancellation,
	target_ref: &str,
	pipeline: u64,
	benchmark_result: Result<(), String>,
	stop_result: Result<(), String>,
) -> Result<(), String> {
	cancellation.check()?;
	let format_benchmark_error =
		|error| format!("Benchmark failed for {target_ref} at pipeline depth {pipeline}: {error}");
	match (benchmark_result, stop_result) {
		(Ok(()), Ok(())) => Ok(()),
		(Err(error), Ok(())) => Err(format_benchmark_error(error)),
		(Ok(()), Err(error)) => Err(error),
		(Err(benchmark_error), Err(stop_error)) => Err(format!(
			"{}\nServer cleanup also failed: {stop_error}",
			format_benchmark_error(benchmark_error)
		)),
	}
}

fn build_full_report(
	config: &Config,
	base: &BenchmarkTarget,
	head: &BenchmarkTarget,
	comparison: &str,
) -> String {
	format!(
		"# Nimbis Redis Benchmark Branch Comparison\n\n\
- Base: `{}` (`{}`)\n\
- Head: `{}` (`{}`)\n\
- Workload: `N={}`, `C={}`, `D={}`, `R={}`, `SEED_N={}`, `seed={}`\n\
- Profiles: `P=1` and `P={}`\n\
- Threads: redis-benchmark `{}`, Nimbis runtime `{}`\n\
- Execution: sequential runs with isolated local file stores\n\n\
{}",
		escape_inline_code(&base.ref_name),
		base.commit,
		escape_inline_code(&head.ref_name),
		head.commit,
		config.requests,
		config.clients,
		config.data_size,
		config.random_keyspace,
		config.seed_requests,
		redis_benchmark::DEFAULT_COMPARISON_SEED,
		config.pipeline_depth,
		optional_value(config.threads, "default"),
		optional_value(config.runtime_threads, "auto"),
		comparison
	)
}

fn optional_value<T: ToString>(value: Option<T>, default: &str) -> String {
	value
		.map(|value| value.to_string())
		.unwrap_or_else(|| default.to_string())
}

fn combine_suite_outputs(suites_dir: &Path, output_path: &Path) -> Result<(), String> {
	let mut paths = fs::read_dir(suites_dir)
		.map_err(|error| format!("Failed to read {}: {error}", suites_dir.display()))?
		.map(|entry| entry.map(|entry| entry.path()))
		.collect::<Result<Vec<_>, _>>()
		.map_err(|error| format!("Failed to read {}: {error}", suites_dir.display()))?;
	paths.retain(|path| path.extension().is_some_and(|extension| extension == "txt"));
	paths.sort();
	if paths.is_empty() {
		return Err(format!(
			"No benchmark output files were written to {}",
			suites_dir.display()
		));
	}

	let mut combined = String::new();
	for path in paths {
		let content = fs::read_to_string(&path)
			.map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
		if content.trim().is_empty() {
			return Err(format!("Benchmark output {} is empty", path.display()));
		}
		combined.push_str(&content);
		if !content.ends_with('\n') {
			combined.push('\n');
		}
	}

	let commands = benchmarks::parse_benchmark(&combined)
		.into_keys()
		.collect::<BTreeSet<_>>();
	let expected = redis_benchmark::COMPARISON_PROFILE_COMMANDS
		.iter()
		.map(|command| (*command).to_string())
		.collect::<BTreeSet<_>>();
	if commands != expected {
		let missing = expected.difference(&commands).cloned().collect::<Vec<_>>();
		let unexpected = commands.difference(&expected).cloned().collect::<Vec<_>>();
		return Err(format!(
			"Incomplete comparison output; missing commands: [{}]; unexpected commands: [{}]",
			missing.join(", "),
			unexpected.join(", ")
		));
	}

	fs::write(output_path, combined)
		.map_err(|error| format!("Failed to write {}: {error}", output_path.display()))?;
	Ok(())
}

pub(crate) struct ServerProcess {
	child: Option<Child>,
	log_path: PathBuf,
}

impl ServerProcess {
	pub(crate) fn start(
		binary: &Path,
		runtime_dir: &Path,
		log_path: &Path,
		port: u16,
		runtime_threads: Option<usize>,
	) -> Result<Self, String> {
		let log = File::create(log_path)
			.map_err(|error| format!("Failed to create {}: {error}", log_path.display()))?;
		let stderr = log
			.try_clone()
			.map_err(|error| format!("Failed to clone {}: {error}", log_path.display()))?;
		let mut command = Command::new(binary);
		command
			.current_dir(runtime_dir)
			.args(["--host", "127.0.0.1", "--port", &port.to_string()])
			.args(["--log-level", "error,nimbis::server=info"])
			.env("NIMBIS_OBJECT_STORE_URL", "file:nimbis_store")
			.env("NIMBIS_TRACE_ENABLED", "false")
			.stdout(Stdio::from(log))
			.stderr(Stdio::from(stderr));
		if let Some(runtime_threads) = runtime_threads {
			command.args(["--runtime-threads", &runtime_threads.to_string()]);
		}
		let child = command.spawn().map_err(|error| {
			format!(
				"Failed to start Nimbis binary {}: {error}",
				binary.display()
			)
		})?;
		Ok(Self {
			child: Some(child),
			log_path: log_path.to_path_buf(),
		})
	}

	pub(crate) fn wait_until_ready(
		&mut self,
		redis_cli: &str,
		host: &str,
		port: u16,
		timeout: Duration,
		cancellation: &Cancellation,
	) -> Result<(), String> {
		let deadline = Instant::now() + timeout;
		let mut last_error = "server was not probed".to_string();
		let mut saw_listening_marker = false;
		while Instant::now() < deadline {
			cancellation.check()?;
			self.ensure_running("before becoming ready")?;

			if !saw_listening_marker {
				match log_contains_listening_marker(&self.log_path, host, port) {
					Ok(true) => saw_listening_marker = true,
					Ok(false) => {
						last_error = "waiting for the Nimbis listening marker".into();
					}
					Err(error) => last_error = error,
				}
				if !saw_listening_marker {
					thread::sleep(
						deadline
							.saturating_duration_since(Instant::now())
							.min(Duration::from_millis(100)),
					);
					continue;
				}
			}

			self.ensure_running("after binding the benchmark port")?;
			let mut probe = Command::new(redis_cli);
			probe.args(["-h", host, "-p", &port.to_string(), "--raw", "PING"]);
			match run_command_until(
				&mut probe,
				deadline,
				cancellation,
				"redis-cli readiness probe",
			) {
				Ok(output) if output.status.success() => {
					cancellation.check()?;
					let response = String::from_utf8_lossy(&output.stdout);
					if response.trim() == "PONG" {
						self.ensure_running("after answering the readiness probe")?;
						return Ok(());
					}
					last_error = format!("unexpected PING response: {}", response.trim());
				}
				Ok(output) => {
					last_error = String::from_utf8_lossy(&output.stderr).trim().to_string();
				}
				Err(error) => {
					cancellation.check()?;
					last_error = error;
				}
			}
			thread::sleep(
				deadline
					.saturating_duration_since(Instant::now())
					.min(Duration::from_millis(100)),
			);
		}

		Err(format!(
			"Nimbis did not become ready at {host}:{port} within {timeout:?}: {last_error}.{}",
			self.log_context()
		))
	}

	fn ensure_running(&mut self, phase: &str) -> Result<(), String> {
		let status = self
			.child
			.as_mut()
			.expect("server child should exist")
			.try_wait()
			.map_err(|error| format!("Failed to inspect Nimbis process: {error}"))?;
		if let Some(status) = status {
			return Err(format!(
				"Nimbis exited {phase} with status {status}.{}",
				self.log_context()
			));
		}
		Ok(())
	}

	pub(crate) fn stop(&mut self) -> Result<(), String> {
		let Some(child) = self.child.as_mut() else {
			return Ok(());
		};
		if let Some(status) = child
			.try_wait()
			.map_err(|error| format!("Failed to inspect Nimbis process: {error}"))?
		{
			self.child = None;
			return Err(format!(
				"Nimbis exited unexpectedly before benchmark cleanup with status {status}.{}",
				self.log_context()
			));
		}
		child
			.kill()
			.map_err(|error| format!("Failed to stop Nimbis process: {error}"))?;
		child
			.wait()
			.map_err(|error| format!("Failed to reap Nimbis process: {error}"))?;
		self.child = None;
		Ok(())
	}

	fn log_context(&self) -> String {
		match fs::read_to_string(&self.log_path) {
			Ok(log) if !log.trim().is_empty() => {
				format!(" Server log ({}):\n{}", self.log_path.display(), log.trim())
			}
			_ => format!(" Server log: {}", self.log_path.display()),
		}
	}
}

fn log_contains_listening_marker(log_path: &Path, host: &str, port: u16) -> Result<bool, String> {
	let log = fs::read_to_string(log_path)
		.map_err(|error| format!("Failed to read {}: {error}", log_path.display()))?;
	Ok(log.contains(&format!("Nimbis server listening on {host}:{port}")))
}

impl Drop for ServerProcess {
	fn drop(&mut self) {
		if let Some(child) = self.child.as_mut() {
			let _ = child.kill();
			let _ = child.wait();
		}
	}
}

fn create_run_dir(output_parent: &Path) -> Result<PathBuf, String> {
	fs::create_dir_all(output_parent)
		.map_err(|error| format!("Failed to create {}: {error}", output_parent.display()))?;
	Builder::new()
		.prefix("run-")
		.tempdir_in(output_parent)
		.map(|directory| directory.keep())
		.map_err(|error| {
			format!(
				"Failed to create a run directory below {}: {error}",
				output_parent.display()
			)
		})
}

pub(crate) fn pick_available_port() -> Result<u16, String> {
	let listener = TcpListener::bind(("127.0.0.1", 0))
		.map_err(|error| format!("Failed to select an available port: {error}"))?;
	listener
		.local_addr()
		.map(|address| address.port())
		.map_err(|error| format!("Failed to inspect selected port: {error}"))
}

pub(crate) fn ensure_port_available(port: u16) -> Result<(), String> {
	TcpListener::bind(("127.0.0.1", port))
		.map(|_| ())
		.map_err(|error| format!("Port {port} is not available on 127.0.0.1: {error}"))
}

fn run_checked(
	command: &mut Command,
	description: &str,
	cancellation: &Cancellation,
) -> Result<(), String> {
	cancellation.check()?;
	let status = command
		.status()
		.map_err(|error| format!("Failed to {description}: {error}"))?;
	cancellation.check()?;
	if status.success() {
		Ok(())
	} else {
		Err(format!(
			"Failed to {description}: process exited with {status}"
		))
	}
}

fn run_command_until(
	command: &mut Command,
	deadline: Instant,
	cancellation: &Cancellation,
	description: &str,
) -> Result<Output, String> {
	cancellation.check()?;
	let mut child = command
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|error| format!("Failed to start {description}: {error}"))?;

	loop {
		if let Err(error) = cancellation.check() {
			stop_child(&mut child);
			return Err(error);
		}
		let now = Instant::now();
		if now >= deadline {
			stop_child(&mut child);
			return Err(format!("{description} exceeded the startup deadline"));
		}
		match child.try_wait() {
			Ok(Some(_)) => {
				return child
					.wait_with_output()
					.map_err(|error| format!("Failed to collect {description} output: {error}"));
			}
			Ok(None) => {}
			Err(error) => {
				stop_child(&mut child);
				return Err(format!("Failed to inspect {description}: {error}"));
			}
		}
		thread::sleep(
			deadline
				.saturating_duration_since(Instant::now())
				.min(Duration::from_millis(20)),
		);
	}
}

fn stop_child(child: &mut Child) {
	let _ = child.kill();
	let _ = child.wait();
}

fn warn_if_worktree_is_dirty(workspace_root: &Path) -> Result<(), String> {
	let output = Command::new("git")
		.current_dir(workspace_root)
		.args(["status", "--porcelain"])
		.output()
		.map_err(|error| format!("Failed to inspect the current worktree: {error}"))?;
	if !output.status.success() {
		return Err(format!(
			"Failed to inspect the current worktree: {}",
			String::from_utf8_lossy(&output.stderr).trim()
		));
	}
	if !output.stdout.is_empty() {
		write_stdout_line(
			"Warning: uncommitted worktree changes are not included in either benchmark ref.",
		)?;
	}
	Ok(())
}

fn short_commit(commit: &str) -> &str {
	commit.get(..12).unwrap_or(commit)
}

fn escape_inline_code(value: &str) -> String {
	value.replace('`', "\\`")
}

#[cfg(test)]
mod tests {
	use tempfile::tempdir;

	use super::*;

	#[test]
	fn explicit_config_uses_ci_comparison_defaults() {
		let root = Path::new("/workspace");
		let config = Config::from_args(
			Args {
				requests: Some(DEFAULT_REQUESTS),
				clients: Some(DEFAULT_CLIENTS),
				data_size: Some(DEFAULT_DATA_SIZE),
				random_keyspace: Some(DEFAULT_RANDOM_KEYSPACE),
				seed_requests: Some(DEFAULT_REQUESTS),
				..Args::default()
			},
			root,
		)
		.unwrap();

		assert_eq!(config.base_ref, "main");
		assert_eq!(config.head_ref, "HEAD");
		assert_eq!(config.requests, 200_000);
		assert_eq!(config.clients, 100);
		assert_eq!(config.data_size, 512);
		assert_eq!(
			config.random_keyspace,
			redis_benchmark::MAX_REDIS_RANDOM_SEED
		);
		assert_eq!(config.pipeline_depth, 50);
	}

	#[test]
	fn config_rejects_zero_workload_values() {
		let error = Config::from_args(
			Args {
				requests: Some(0),
				..Args::default()
			},
			Path::new("/workspace"),
		)
		.unwrap_err();

		assert_eq!(error, "N must be greater than zero");
	}

	#[test]
	fn combines_all_comparison_suite_outputs_in_order() {
		let dir = tempdir().unwrap();
		let suites = dir.path().join("suites");
		fs::create_dir(&suites).unwrap();
		fs::write(
			suites.join("builtin_comparison.txt"),
			concat!(
				"SET: 100 requests per second\n",
				"GET: 101 requests per second\n",
				"LPUSH: 102 requests per second\n",
				"LPOP: 103 requests per second\n",
				"SADD: 104 requests per second\n",
				"HSET: 105 requests per second\n",
				"ZADD: 106 requests per second\n",
			),
		)
		.unwrap();
		for (filename, command) in [
			("hget.txt", "HGET"),
			("srem.txt", "SREM"),
			("zrem.txt", "ZREM"),
		] {
			fs::write(
				suites.join(filename),
				format!("{command}: 200 requests per second\n"),
			)
			.unwrap();
		}
		let output = dir.path().join("combined.txt");

		combine_suite_outputs(&suites, &output).unwrap();

		let combined = fs::read_to_string(output).unwrap();
		assert!(combined.starts_with("SET: 100 requests per second"));
		assert!(combined.contains("HGET: 200 requests per second"));
		assert!(combined.ends_with("ZREM: 200 requests per second\n"));
	}

	#[test]
	fn combine_rejects_incomplete_comparison_output() {
		let dir = tempdir().unwrap();
		let suites = dir.path().join("suites");
		fs::create_dir(&suites).unwrap();
		fs::write(
			suites.join("builtin_comparison.txt"),
			"SET: 100 requests per second\n",
		)
		.unwrap();

		let error = combine_suite_outputs(&suites, &dir.path().join("combined.txt")).unwrap_err();

		assert!(error.contains("Incomplete comparison output"));
		assert!(error.contains("GET"));
	}

	#[test]
	fn full_report_identifies_refs_commits_and_workload() {
		let config = Config::from_args(
			Args {
				requests: Some(100),
				clients: Some(2),
				data_size: Some(16),
				random_keyspace: Some(10),
				seed_requests: Some(20),
				threads: Some(3),
				runtime_threads: Some(4),
				..Args::default()
			},
			Path::new("/workspace"),
		)
		.unwrap();
		let base = BenchmarkTarget {
			ref_name: "main".into(),
			commit: "aaaaaaaaaaaaaaaa".into(),
			binary: PathBuf::from("base"),
		};
		let head = BenchmarkTarget {
			ref_name: "feature/perf".into(),
			commit: "bbbbbbbbbbbbbbbb".into(),
			binary: PathBuf::from("head"),
		};

		let report = build_full_report(&config, &base, &head, "comparison\n");

		assert!(report.contains("Base: `main` (`aaaaaaaaaaaaaaaa`)"));
		assert!(report.contains("Head: `feature/perf` (`bbbbbbbbbbbbbbbb`)"));
		assert!(report.contains("`N=100`, `C=2`, `D=16`, `R=10`, `SEED_N=20`, `seed=279000`"));
		assert!(report.contains("Profiles: `P=1` and `P=50`"));
		assert!(report.contains("redis-benchmark `3`, Nimbis runtime `4`"));
	}

	#[test]
	fn listening_marker_matches_the_expected_address() {
		let dir = tempdir().unwrap();
		let log_path = dir.path().join("server.log");
		fs::write(
			&log_path,
			"INFO nimbis::server: Nimbis server listening on 127.0.0.1:6380\n",
		)
		.unwrap();

		assert!(log_contains_listening_marker(&log_path, "127.0.0.1", 6380).unwrap());
		assert!(!log_contains_listening_marker(&log_path, "127.0.0.1", 6381).unwrap());
	}

	#[test]
	fn cancellation_reports_an_interrupted_comparison() {
		let cancellation = Cancellation::new();
		assert!(cancellation.check().is_ok());

		cancellation.request();

		assert_eq!(
			cancellation.check().unwrap_err(),
			"Benchmark comparison interrupted"
		);
	}

	#[test]
	fn build_cache_lock_wait_can_be_cancelled() {
		let dir = tempdir().unwrap();
		let first = lock_build_cache(dir.path(), &Cancellation::new()).unwrap();
		let cancellation = Cancellation::new();
		let signal_cancellation = cancellation.clone();
		let signal = thread::spawn(move || {
			thread::sleep(Duration::from_millis(20));
			signal_cancellation.request();
		});

		let error = lock_build_cache(dir.path(), &cancellation).unwrap_err();
		signal.join().unwrap();
		drop(first);

		assert_eq!(error, "Benchmark comparison interrupted");
	}

	#[test]
	fn benchmark_and_server_failures_are_both_reported() {
		let error = finish_benchmark_pass(
			&Cancellation::new(),
			"feature/perf",
			50,
			Err("redis-benchmark disconnected".into()),
			Err("Nimbis exited unexpectedly. Server log: bind failed".into()),
		)
		.unwrap_err();

		assert!(error.contains("Benchmark failed for feature/perf at pipeline depth 50"));
		assert!(error.contains("redis-benchmark disconnected"));
		assert!(error.contains("Server cleanup also failed"));
		assert!(error.contains("Server log: bind failed"));
	}

	#[test]
	fn readiness_probe_honors_the_startup_deadline() {
		let mut command = Command::new(env::current_exe().unwrap());
		command.args([
			"--ignored",
			"--exact",
			"branch_benchmark::tests::hanging_probe_child",
		]);
		let started = Instant::now();

		let error = run_command_until(
			&mut command,
			started + Duration::from_millis(100),
			&Cancellation::new(),
			"test readiness probe",
		)
		.unwrap_err();

		assert_eq!(error, "test readiness probe exceeded the startup deadline");
		assert!(started.elapsed() < Duration::from_secs(2));
	}

	#[test]
	#[ignore = "helper process for readiness_probe_honors_the_startup_deadline"]
	fn hanging_probe_child() {
		thread::sleep(Duration::from_secs(10));
	}
}
