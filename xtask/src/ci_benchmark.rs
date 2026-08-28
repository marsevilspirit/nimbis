use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use clap::Args as ClapArgs;
use serde::Deserialize;
use serde::Serialize;
use walkdir::WalkDir;

use crate::benchmarks;
use crate::branch_benchmark::Cancellation;
use crate::branch_benchmark::ServerProcess;
use crate::branch_benchmark::ensure_port_available;
use crate::branch_benchmark::pick_available_port;
use crate::redis_benchmark;
use crate::redis_benchmark::ComparisonCommand;
use crate::write_stdout;
use crate::write_stdout_line;

const SCHEMA_VERSION: u32 = 1;
const DEFAULT_REQUESTS: u64 = 200_000;
const DEFAULT_CLIENTS: u64 = 100;
const DEFAULT_RANDOM_KEYSPACE: u64 = 100_000;
const DEFAULT_PIPELINE_DEPTH: u64 = 50;
const DEFAULT_STARTUP_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_SETTLE_MILLIS: u64 = 1_000;
const P1_RPS_MATERIALITY_PERCENT: f64 = 5.0;
const PIPELINE_RPS_MATERIALITY_PERCENT: f64 = 8.0;
const P1_DUPLICATE_SPREAD_LIMIT_PERCENT: f64 = 10.0;
const PIPELINE_DUPLICATE_SPREAD_LIMIT_PERCENT: f64 = 16.0;

#[derive(ClapArgs, Debug)]
pub struct ShardArgs {
	/// Main/base Nimbis release binary.
	#[arg(long)]
	main_binary: PathBuf,

	/// PR/candidate Nimbis release binary.
	#[arg(long)]
	pr_binary: PathBuf,

	/// Commands in this shard. Commands run sequentially with fresh stores.
	#[arg(long, value_delimiter = ',', required = true)]
	commands: Vec<ComparisonCommand>,

	/// Payload size.
	#[arg(long)]
	data_size: u64,

	/// Independent runner replica number, starting at one.
	#[arg(long)]
	replica: u64,

	/// GitHub Actions run attempt used to select the newest rerun artifact.
	#[arg(long, default_value_t = 1)]
	run_attempt: u64,

	/// Request count per benchmark pass.
	#[arg(long, default_value_t = DEFAULT_REQUESTS)]
	requests: u64,

	/// Concurrent redis-benchmark clients.
	#[arg(long, default_value_t = DEFAULT_CLIENTS)]
	clients: u64,

	/// Random key space.
	#[arg(long, default_value_t = DEFAULT_RANDOM_KEYSPACE)]
	random_keyspace: u64,

	/// Pipeline depth used for throughput screening.
	#[arg(long, default_value_t = DEFAULT_PIPELINE_DEPTH)]
	pipeline_depth: u64,

	/// Optional redis-benchmark --threads value.
	#[arg(long)]
	threads: Option<u64>,

	/// Optional Nimbis Tokio runtime worker count.
	#[arg(long)]
	runtime_threads: Option<usize>,

	/// Setup request count. Defaults to the measured request count.
	#[arg(long = "seed-n")]
	seed_requests: Option<u64>,

	/// Stable seed namespace. Each cell derives a distinct matched seed.
	#[arg(long, default_value_t = 277_000)]
	seed_base: u64,

	/// Milliseconds to let seeded state settle before measurement.
	#[arg(long, default_value_t = DEFAULT_SETTLE_MILLIS)]
	settle_millis: u64,

	/// Seconds to wait for bind, PONG, and child-alive evidence.
	#[arg(long, default_value_t = DEFAULT_STARTUP_TIMEOUT_SECONDS)]
	startup_timeout_seconds: u64,

	/// Redis 8 redis-benchmark binary.
	#[arg(long, default_value = "redis-benchmark")]
	redis_benchmark: String,

	/// redis-cli binary used for readiness checks.
	#[arg(long, default_value = "redis-cli")]
	redis_cli: String,

	/// Reader-facing label for the base binary.
	#[arg(long, default_value = "Main")]
	main_label: String,

	/// Reader-facing label for the candidate binary.
	#[arg(long, default_value = "PR")]
	pr_label: String,

	/// Directory for raw pass output, logs, and result.json.
	#[arg(long)]
	output_dir: PathBuf,
}

#[derive(ClapArgs, Debug)]
pub struct ReportArgs {
	/// Directory containing downloaded shard artifacts.
	#[arg(long)]
	input_dir: PathBuf,

	/// Markdown report path.
	#[arg(long)]
	output: PathBuf,

	/// Required independent replicas for every command/configuration cell.
	#[arg(long, default_value_t = 3)]
	expected_replicas: u64,

	/// Required payload sizes.
	#[arg(long, value_delimiter = ',', default_value = "512,1024")]
	expected_data_sizes: Vec<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Branch {
	Main,
	Pr,
}

impl Branch {
	fn label(self) -> &'static str {
		match self {
			Self::Main => "main",
			Self::Pr => "pr",
		}
	}
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PassResult {
	position: usize,
	branch: Branch,
	rps: f64,
	p50_msec: Option<f64>,
	artifact_dir: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct MetricEffect {
	main_geomean: f64,
	pr_geomean: f64,
	delta_percent: f64,
	absolute_delta: f64,
	main_duplicate_spread_percent: f64,
	pr_duplicate_spread_percent: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BlockResult {
	command: String,
	pipeline_depth: u64,
	order: String,
	seed: u64,
	passes: Vec<PassResult>,
	rps: MetricEffect,
	p50_msec: Option<MetricEffect>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ShardResult {
	schema_version: u32,
	main_label: String,
	pr_label: String,
	data_size: u64,
	replica: u64,
	run_attempt: u64,
	commands: Vec<String>,
	requests: u64,
	clients: u64,
	random_keyspace: u64,
	pipeline_depth: u64,
	threads: Option<u64>,
	runtime_threads: Option<usize>,
	seed_requests: u64,
	seed_base: u64,
	settle_millis: u64,
	blocks: Vec<BlockResult>,
}

pub fn run_shard(args: ShardArgs, workspace_root: &Path) -> Result<(), String> {
	validate_shard_args(&args)?;
	redis_benchmark::require_cmd(&args.redis_benchmark)?;
	redis_benchmark::require_cmd(&args.redis_cli)?;

	let cancellation = Cancellation::install()?;
	let runtime_root = tempfile::Builder::new()
		.prefix("nimbis-ci-benchmark-")
		.tempdir()
		.map_err(|error| format!("Failed to create temporary runtime directory: {error}"))?;
	fs::create_dir_all(&args.output_dir).map_err(|error| {
		format!(
			"Failed to create benchmark output directory {}: {error}",
			args.output_dir.display()
		)
	})?;
	if args.output_dir.join("result.json").exists() {
		return Err(format!(
			"Refusing to mix a new shard with existing {}",
			args.output_dir.join("result.json").display()
		));
	}

	let mut commands = args.commands.clone();
	let mut pipeline_depths = vec![1, args.pipeline_depth];
	if args.replica.is_multiple_of(2) {
		commands.reverse();
		pipeline_depths.reverse();
	}

	write_stdout_line(&format!(
		"Benchmark CI shard: commands={} data_size={} replica={}",
		commands
			.iter()
			.map(|command| command.as_str())
			.collect::<Vec<_>>()
			.join(","),
		args.data_size,
		args.replica
	))?;
	write_stdout_line(
		"Commands and pipeline modes run sequentially; every pass uses a fresh process and store.",
	)?;

	let mut blocks = Vec::new();
	for command in commands {
		for &pipeline_depth in &pipeline_depths {
			cancellation.check()?;
			blocks.push(run_block(
				&args,
				workspace_root,
				command,
				pipeline_depth,
				runtime_root.path(),
				&cancellation,
			)?);
		}
	}

	let result = ShardResult {
		schema_version: SCHEMA_VERSION,
		main_label: args.main_label,
		pr_label: args.pr_label,
		data_size: args.data_size,
		replica: args.replica,
		run_attempt: args.run_attempt,
		commands: args
			.commands
			.iter()
			.map(|command| command.as_str().to_string())
			.collect(),
		requests: args.requests,
		clients: args.clients,
		random_keyspace: args.random_keyspace,
		pipeline_depth: args.pipeline_depth,
		threads: args.threads,
		runtime_threads: args.runtime_threads,
		seed_requests: args.seed_requests.unwrap_or(args.requests),
		seed_base: args.seed_base,
		settle_millis: args.settle_millis,
		blocks,
	};
	let result_path = args.output_dir.join("result.json");
	let json = serde_json::to_string_pretty(&result)
		.map_err(|error| format!("Failed to encode benchmark shard: {error}"))?;
	fs::write(&result_path, format!("{json}\n"))
		.map_err(|error| format!("Failed to write {}: {error}", result_path.display()))?;
	write_stdout_line(&format!("Shard result: {}", result_path.display()))?;
	Ok(())
}

fn validate_shard_args(args: &ShardArgs) -> Result<(), String> {
	for (name, value) in [
		("data size", args.data_size),
		("replica", args.replica),
		("run attempt", args.run_attempt),
		("requests", args.requests),
		("clients", args.clients),
		("random keyspace", args.random_keyspace),
		("pipeline depth", args.pipeline_depth),
		("startup timeout", args.startup_timeout_seconds),
	] {
		if value == 0 {
			return Err(format!("{name} must be greater than zero"));
		}
	}
	if args.pipeline_depth == 1 {
		return Err("pipeline depth must be greater than one".into());
	}
	if args.runtime_threads == Some(0) {
		return Err("runtime threads must be greater than zero".into());
	}
	if !args.main_binary.is_file() {
		return Err(format!(
			"Main binary was not found at {}",
			args.main_binary.display()
		));
	}
	if !args.pr_binary.is_file() {
		return Err(format!(
			"PR binary was not found at {}",
			args.pr_binary.display()
		));
	}
	let unique = args
		.commands
		.iter()
		.map(|command| command.as_str())
		.collect::<BTreeSet<_>>();
	if unique.len() != args.commands.len() {
		return Err("commands must not contain duplicates".into());
	}
	Ok(())
}

fn run_block(
	args: &ShardArgs,
	workspace_root: &Path,
	command: ComparisonCommand,
	pipeline_depth: u64,
	runtime_root: &Path,
	cancellation: &Cancellation,
) -> Result<BlockResult, String> {
	let order = block_order(args.replica);
	let order_label = order
		.iter()
		.map(|branch| match branch {
			Branch::Main => 'A',
			Branch::Pr => 'B',
		})
		.collect::<String>();
	let seed = derive_seed(
		args.seed_base,
		command,
		args.data_size,
		pipeline_depth,
		args.replica,
	);
	write_stdout_line(&format!(
		"\n==> {} D={} P={} replica={} order={} seed={}",
		command.as_str(),
		args.data_size,
		pipeline_depth,
		args.replica,
		order_label,
		seed
	))?;

	let mut passes = Vec::new();
	for (position, branch) in order.into_iter().enumerate() {
		passes.push(run_pass(
			args,
			workspace_root,
			command,
			pipeline_depth,
			seed,
			position + 1,
			branch,
			runtime_root,
			cancellation,
		)?);
	}

	let rps = metric_effect(&passes, |pass| Some(pass.rps))?
		.ok_or_else(|| "RPS was unexpectedly absent".to_string())?;
	let p50_msec = metric_effect(&passes, |pass| pass.p50_msec)?;
	Ok(BlockResult {
		command: command.as_str().to_string(),
		pipeline_depth,
		order: order_label,
		seed,
		passes,
		rps,
		p50_msec,
	})
}

#[allow(clippy::too_many_arguments)]
fn run_pass(
	args: &ShardArgs,
	workspace_root: &Path,
	command: ComparisonCommand,
	pipeline_depth: u64,
	seed: u64,
	position: usize,
	branch: Branch,
	runtime_root: &Path,
	cancellation: &Cancellation,
) -> Result<PassResult, String> {
	cancellation.check()?;
	let binary = match branch {
		Branch::Main => &args.main_binary,
		Branch::Pr => &args.pr_binary,
	};
	let binary = fs::canonicalize(binary)
		.map_err(|error| format!("Failed to resolve {}: {error}", binary.display()))?;
	let pass_name = format!(
		"{}-d{}-p{}-r{}-{}-{}",
		command.as_str().to_ascii_lowercase(),
		args.data_size,
		pipeline_depth,
		args.replica,
		position,
		branch.label()
	);
	let artifact_dir = args.output_dir.join("raw").join(&pass_name);
	let runtime_dir = tempfile::Builder::new()
		.prefix(&format!("{pass_name}-"))
		.tempdir_in(runtime_root)
		.map_err(|error| format!("Failed to create runtime directory for {pass_name}: {error}"))?;
	let suites_dir = artifact_dir.join("suites");
	fs::create_dir_all(&suites_dir)
		.map_err(|error| format!("Failed to create {}: {error}", suites_dir.display()))?;

	let port = pick_available_port()?;
	ensure_port_available(port)?;
	let log_path = artifact_dir.join("server.log");
	let mut server = ServerProcess::start(
		&binary,
		runtime_dir.path(),
		&log_path,
		port,
		args.runtime_threads,
	)?;
	server.wait_until_ready(
		&args.redis_cli,
		"127.0.0.1",
		port,
		Duration::from_secs(args.startup_timeout_seconds),
		cancellation,
	)?;
	cancellation.check()?;

	let benchmark_result = redis_benchmark::run(
		redis_benchmark::Args {
			host: Some("127.0.0.1".into()),
			port: Some(port),
			requests: Some(args.requests),
			clients: Some(args.clients),
			data_size: Some(args.data_size),
			pipeline: Some(pipeline_depth),
			random_keyspace: Some(args.random_keyspace),
			threads: args.threads,
			csv: false,
			force_quiet: true,
			output_dir: Some(suites_dir.display().to_string()),
			seed_requests: Some(args.seed_requests.unwrap_or(args.requests)),
			command: Some(command),
			seed: Some(seed),
			settle_millis: Some(args.settle_millis),
			redis_benchmark: Some(args.redis_benchmark.clone()),
			redis_cli: Some(args.redis_cli.clone()),
			extra_args: Vec::new(),
			profile: redis_benchmark::Profile::Comparison,
		},
		workspace_root,
	);
	let stop_result = server.stop();
	match (benchmark_result, stop_result) {
		(Ok(()), Ok(())) => {}
		(Err(error), Ok(())) => return Err(error),
		(Ok(()), Err(error)) => return Err(error),
		(Err(benchmark_error), Err(stop_error)) => {
			return Err(format!(
				"{benchmark_error}\nServer cleanup also failed: {stop_error}"
			));
		}
	}
	cancellation.check()?;

	let result = read_single_command_result(&suites_dir, command)?;
	runtime_dir
		.close()
		.map_err(|error| format!("Failed to remove runtime store for {pass_name}: {error}"))?;
	let relative_artifact = artifact_dir
		.strip_prefix(&args.output_dir)
		.unwrap_or(&artifact_dir)
		.display()
		.to_string();
	Ok(PassResult {
		position,
		branch,
		rps: result.rps,
		p50_msec: result.p50_msec,
		artifact_dir: relative_artifact,
	})
}

fn read_single_command_result(
	suites_dir: &Path,
	command: ComparisonCommand,
) -> Result<benchmarks::BenchmarkResult, String> {
	let mut combined = String::new();
	for entry in fs::read_dir(suites_dir)
		.map_err(|error| format!("Failed to read {}: {error}", suites_dir.display()))?
	{
		let path = entry
			.map_err(|error| format!("Failed to read {}: {error}", suites_dir.display()))?
			.path();
		if path.extension().is_some_and(|extension| extension == "txt") {
			combined.push_str(
				&fs::read_to_string(&path)
					.map_err(|error| format!("Failed to read {}: {error}", path.display()))?,
			);
			combined.push('\n');
		}
	}
	let mut parsed = benchmarks::parse_benchmark(&combined);
	let expected = command.as_str();
	let result = parsed
		.remove(expected)
		.or_else(|| {
			parsed
				.keys()
				.find(|key| key.eq_ignore_ascii_case(expected))
				.cloned()
				.and_then(|key| parsed.remove(&key))
		})
		.ok_or_else(|| {
			format!(
				"No parseable {} result found in {}",
				expected,
				suites_dir.display()
			)
		})?;
	if !parsed.is_empty() {
		return Err(format!(
			"Expected only {}, but benchmark output also contained: {}",
			expected,
			parsed.keys().cloned().collect::<Vec<_>>().join(", ")
		));
	}
	Ok(result)
}

fn block_order(replica: u64) -> [Branch; 4] {
	if replica.is_multiple_of(2) {
		[Branch::Pr, Branch::Main, Branch::Main, Branch::Pr]
	} else {
		[Branch::Main, Branch::Pr, Branch::Pr, Branch::Main]
	}
}

fn derive_seed(
	base: u64,
	command: ComparisonCommand,
	data_size: u64,
	pipeline_depth: u64,
	replica: u64,
) -> u64 {
	let mut hash = 0xcbf2_9ce4_8422_2325_u64 ^ base;
	for byte in command.as_str().bytes() {
		hash ^= u64::from(byte);
		hash = hash.wrapping_mul(0x100_0000_01b3);
	}
	for value in [data_size, pipeline_depth, replica] {
		hash ^= value;
		hash = hash.wrapping_mul(0x100_0000_01b3);
	}
	(hash % redis_benchmark::MAX_REDIS_RANDOM_SEED) + 1
}

fn metric_effect<F>(passes: &[PassResult], value: F) -> Result<Option<MetricEffect>, String>
where
	F: Fn(&PassResult) -> Option<f64>,
{
	let main = passes
		.iter()
		.filter(|pass| pass.branch == Branch::Main)
		.map(&value)
		.collect::<Option<Vec<_>>>();
	let pr = passes
		.iter()
		.filter(|pass| pass.branch == Branch::Pr)
		.map(value)
		.collect::<Option<Vec<_>>>();
	let (Some(main), Some(pr)) = (main, pr) else {
		return Ok(None);
	};
	if main.len() != 2 || pr.len() != 2 {
		return Err("A paired block must contain two Main and two PR measurements".into());
	}
	if main.iter().chain(&pr).any(|value| *value <= 0.0) {
		return Err("Benchmark metrics must be greater than zero".into());
	}
	let main_geomean = geometric_mean(&main);
	let pr_geomean = geometric_mean(&pr);
	Ok(Some(MetricEffect {
		main_geomean,
		pr_geomean,
		delta_percent: ((pr_geomean / main_geomean) - 1.0) * 100.0,
		absolute_delta: pr_geomean - main_geomean,
		main_duplicate_spread_percent: symmetric_spread_percent(main[0], main[1]),
		pr_duplicate_spread_percent: symmetric_spread_percent(pr[0], pr[1]),
	}))
}

fn geometric_mean(values: &[f64]) -> f64 {
	(values.iter().map(|value| value.ln()).sum::<f64>() / values.len() as f64).exp()
}

fn symmetric_spread_percent(first: f64, second: f64) -> f64 {
	((first / second).ln().abs().exp() - 1.0) * 100.0
}

pub fn report(args: ReportArgs) -> Result<(), String> {
	if args.expected_replicas == 0 {
		return Err("expected replicas must be greater than zero".into());
	}
	if args.expected_data_sizes.is_empty()
		|| args.expected_data_sizes.contains(&0)
		|| args
			.expected_data_sizes
			.iter()
			.collect::<BTreeSet<_>>()
			.len() != args.expected_data_sizes.len()
	{
		return Err("expected data sizes must be unique and greater than zero".into());
	}
	let shards = read_shards(&args.input_dir)?;
	let report = build_report(&shards, args.expected_replicas, &args.expected_data_sizes)?;
	if let Some(parent) = args.output.parent() {
		fs::create_dir_all(parent)
			.map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;
	}
	fs::write(&args.output, &report)
		.map_err(|error| format!("Failed to write {}: {error}", args.output.display()))?;
	write_stdout(&report)?;
	Ok(())
}

fn read_shards(input_dir: &Path) -> Result<Vec<ShardResult>, String> {
	let mut paths = WalkDir::new(input_dir)
		.into_iter()
		.filter_map(Result::ok)
		.filter(|entry| entry.file_type().is_file() && entry.file_name() == "result.json")
		.map(|entry| entry.into_path())
		.collect::<Vec<_>>();
	paths.sort();
	if paths.is_empty() {
		return Err(format!(
			"No benchmark shard result.json files found below {}",
			input_dir.display()
		));
	}
	let shards = paths
		.into_iter()
		.map(|path| {
			let content = fs::read_to_string(&path)
				.map_err(|error| format!("Failed to read {}: {error}", path.display()))?;
			let shard: ShardResult = serde_json::from_str(&content)
				.map_err(|error| format!("Failed to parse {}: {error}", path.display()))?;
			if shard.schema_version != SCHEMA_VERSION {
				return Err(format!(
					"Unsupported benchmark schema {} in {}",
					shard.schema_version,
					path.display()
				));
			}
			validate_shard_result(&shard, &path)?;
			Ok(shard)
		})
		.collect::<Result<Vec<_>, String>>()?;
	let mut newest = BTreeMap::new();
	for shard in shards {
		let mut commands = shard.commands.clone();
		commands.sort();
		let key = (shard.data_size, shard.replica, commands.join(","));
		match newest.entry(key) {
			std::collections::btree_map::Entry::Vacant(entry) => {
				entry.insert(shard);
			}
			std::collections::btree_map::Entry::Occupied(mut entry) => {
				if shard.run_attempt > entry.get().run_attempt {
					entry.insert(shard);
				} else if shard.run_attempt == entry.get().run_attempt {
					return Err(format!(
						"Duplicate shard for D={} replica={} commands={} attempt={}",
						entry.key().0,
						entry.key().1,
						entry.key().2,
						shard.run_attempt
					));
				}
			}
		}
	}
	Ok(newest.into_values().collect())
}

fn validate_shard_result(shard: &ShardResult, path: &Path) -> Result<(), String> {
	for (name, value) in [
		("data size", shard.data_size),
		("replica", shard.replica),
		("run attempt", shard.run_attempt),
		("requests", shard.requests),
		("clients", shard.clients),
		("random keyspace", shard.random_keyspace),
		("pipeline depth", shard.pipeline_depth),
		("seed requests", shard.seed_requests),
	] {
		if value == 0 {
			return Err(format!(
				"Invalid {name} in benchmark shard {}",
				path.display()
			));
		}
	}
	if shard.pipeline_depth == 1 {
		return Err(format!(
			"Pipeline depth must be greater than one in {}",
			path.display()
		));
	}
	if shard.threads == Some(0) || shard.runtime_threads == Some(0) {
		return Err(format!(
			"Thread counts must be greater than zero in {}",
			path.display()
		));
	}

	let commands = shard
		.commands
		.iter()
		.map(String::as_str)
		.collect::<BTreeSet<_>>();
	if commands.is_empty() || commands.len() != shard.commands.len() {
		return Err(format!(
			"Shard commands must be non-empty and unique in {}",
			path.display()
		));
	}
	if let Some(command) = commands
		.iter()
		.find(|command| comparison_command(command).is_none())
	{
		return Err(format!(
			"Unsupported command {command} in {}",
			path.display()
		));
	}

	let expected_order = block_order(shard.replica);
	let expected_order_label = expected_order
		.iter()
		.map(|branch| match branch {
			Branch::Main => 'A',
			Branch::Pr => 'B',
		})
		.collect::<String>();
	let mut cells = BTreeSet::new();
	for block in &shard.blocks {
		let command = comparison_command(&block.command).ok_or_else(|| {
			format!(
				"Unsupported block command {} in {}",
				block.command,
				path.display()
			)
		})?;
		if !commands.contains(block.command.as_str())
			|| ![1, shard.pipeline_depth].contains(&block.pipeline_depth)
		{
			return Err(format!(
				"Unexpected block {} P={} in {}",
				block.command,
				block.pipeline_depth,
				path.display()
			));
		}
		if !cells.insert((block.command.clone(), block.pipeline_depth)) {
			return Err(format!(
				"Duplicate block {} P={} in {}",
				block.command,
				block.pipeline_depth,
				path.display()
			));
		}
		if block.order != expected_order_label {
			return Err(format!(
				"Block {} P={} has order {}, expected {} in {}",
				block.command,
				block.pipeline_depth,
				block.order,
				expected_order_label,
				path.display()
			));
		}
		let expected_seed = derive_seed(
			shard.seed_base,
			command,
			shard.data_size,
			block.pipeline_depth,
			shard.replica,
		);
		if block.seed != expected_seed {
			return Err(format!(
				"Block {} P={} has seed {}, expected {} in {}",
				block.command,
				block.pipeline_depth,
				block.seed,
				expected_seed,
				path.display()
			));
		}
		if block.passes.len() != expected_order.len() {
			return Err(format!(
				"Block {} P={} must contain four passes in {}",
				block.command,
				block.pipeline_depth,
				path.display()
			));
		}
		for (index, pass) in block.passes.iter().enumerate() {
			if pass.position != index + 1 || pass.branch != expected_order[index] {
				return Err(format!(
					"Block {} P={} has an invalid pass sequence in {}",
					block.command,
					block.pipeline_depth,
					path.display()
				));
			}
			if !pass.rps.is_finite()
				|| pass.rps <= 0.0
				|| pass
					.p50_msec
					.is_some_and(|value| !value.is_finite() || value <= 0.0)
				|| pass.artifact_dir.trim().is_empty()
			{
				return Err(format!(
					"Block {} P={} contains an invalid pass metric or artifact path in {}",
					block.command,
					block.pipeline_depth,
					path.display()
				));
			}
		}

		let recomputed_rps = metric_effect(&block.passes, |pass| Some(pass.rps))?
			.ok_or_else(|| "Validated RPS passes did not produce an effect".to_string())?;
		if !metric_effect_matches(&block.rps, &recomputed_rps) {
			return Err(format!(
				"Block {} P={} has an RPS effect inconsistent with its passes in {}",
				block.command,
				block.pipeline_depth,
				path.display()
			));
		}
		let recomputed_p50 = metric_effect(&block.passes, |pass| pass.p50_msec)?;
		if !optional_metric_effect_matches(block.p50_msec.as_ref(), recomputed_p50.as_ref()) {
			return Err(format!(
				"Block {} P={} has a p50 effect inconsistent with its passes in {}",
				block.command,
				block.pipeline_depth,
				path.display()
			));
		}
	}

	let expected_cells = commands
		.iter()
		.flat_map(|command| {
			[1, shard.pipeline_depth]
				.into_iter()
				.map(move |pipeline_depth| ((*command).to_string(), pipeline_depth))
		})
		.collect::<BTreeSet<_>>();
	if cells != expected_cells {
		return Err(format!(
			"Shard blocks do not match declared commands in {}",
			path.display()
		));
	}
	Ok(())
}

fn comparison_command(command: &str) -> Option<ComparisonCommand> {
	match command {
		"GET" => Some(ComparisonCommand::Get),
		"SET" => Some(ComparisonCommand::Set),
		"HGET" => Some(ComparisonCommand::Hget),
		"HSET" => Some(ComparisonCommand::Hset),
		"LPUSH" => Some(ComparisonCommand::Lpush),
		"LPOP" => Some(ComparisonCommand::Lpop),
		"SADD" => Some(ComparisonCommand::Sadd),
		"SREM" => Some(ComparisonCommand::Srem),
		"ZADD" => Some(ComparisonCommand::Zadd),
		"ZREM" => Some(ComparisonCommand::Zrem),
		_ => None,
	}
}

fn optional_metric_effect_matches(
	actual: Option<&MetricEffect>,
	expected: Option<&MetricEffect>,
) -> bool {
	match (actual, expected) {
		(Some(actual), Some(expected)) => metric_effect_matches(actual, expected),
		(None, None) => true,
		_ => false,
	}
}

fn metric_effect_matches(actual: &MetricEffect, expected: &MetricEffect) -> bool {
	[
		(actual.main_geomean, expected.main_geomean),
		(actual.pr_geomean, expected.pr_geomean),
		(actual.delta_percent, expected.delta_percent),
		(actual.absolute_delta, expected.absolute_delta),
		(
			actual.main_duplicate_spread_percent,
			expected.main_duplicate_spread_percent,
		),
		(
			actual.pr_duplicate_spread_percent,
			expected.pr_duplicate_spread_percent,
		),
	]
	.into_iter()
	.all(|(actual, expected)| {
		actual.is_finite()
			&& expected.is_finite()
			&& (actual - expected).abs() <= 1e-9 * actual.abs().max(expected.abs()).max(1.0)
	})
}

fn build_report(
	shards: &[ShardResult],
	expected_replicas: u64,
	expected_data_sizes: &[u64],
) -> Result<String, String> {
	let first = shards
		.first()
		.ok_or_else(|| "At least one benchmark shard is required".to_string())?;
	if shards.iter().any(|shard| {
		shard.main_label != first.main_label
			|| shard.pr_label != first.pr_label
			|| shard.requests != first.requests
			|| shard.clients != first.clients
			|| shard.random_keyspace != first.random_keyspace
			|| shard.pipeline_depth != first.pipeline_depth
			|| shard.threads != first.threads
			|| shard.runtime_threads != first.runtime_threads
			|| shard.seed_requests != first.seed_requests
			|| shard.seed_base != first.seed_base
			|| shard.settle_millis != first.settle_millis
	}) {
		return Err("Shard metadata does not describe one comparable benchmark run".into());
	}

	let mut groups: BTreeMap<(String, u64, u64), BTreeMap<u64, &BlockResult>> = BTreeMap::new();
	for shard in shards {
		for block in &shard.blocks {
			let key = (block.command.clone(), shard.data_size, block.pipeline_depth);
			if groups
				.entry(key.clone())
				.or_default()
				.insert(shard.replica, block)
				.is_some()
			{
				return Err(format!(
					"Duplicate replica {} for {} D={} P={}",
					shard.replica, key.0, key.1, key.2
				));
			}
		}
	}
	let expected_commands = redis_benchmark::COMPARISON_PROFILE_COMMANDS
		.iter()
		.map(|command| (*command).to_string())
		.collect::<BTreeSet<_>>();
	let expected_sizes = expected_data_sizes.iter().copied().collect::<BTreeSet<_>>();
	for (command, data_size, pipeline_depth) in groups.keys() {
		if !expected_commands.contains(command)
			|| !expected_sizes.contains(data_size)
			|| ![1, first.pipeline_depth].contains(pipeline_depth)
		{
			return Err(format!(
				"Unexpected benchmark cell {command} D={data_size} P={pipeline_depth}"
			));
		}
	}
	for command in &expected_commands {
		for &data_size in &expected_sizes {
			for pipeline_depth in [1, first.pipeline_depth] {
				if !groups.contains_key(&(command.clone(), data_size, pipeline_depth)) {
					return Err(format!(
						"Missing benchmark cell {command} D={data_size} P={pipeline_depth}"
					));
				}
			}
		}
	}
	let expected = (1..=expected_replicas).collect::<BTreeSet<_>>();
	for ((command, data_size, pipeline_depth), replicas) in &groups {
		let actual = replicas.keys().copied().collect::<BTreeSet<_>>();
		if actual != expected {
			return Err(format!(
				"Incomplete replicas for {command} D={data_size} P={pipeline_depth}; expected {:?}, found {:?}",
				expected, actual
			));
		}
	}

	let mut report = format!(
		"# Nimbis paired benchmark screening\n\n\
- Base: `{}`\n\
- Candidate: `{}`\n\
- Workload: `N={}`, `C={}`, `R={}`\n\
- Replicas per cell: `{}` independent runners\n\
- Workflow attempts represented: `{}`\n\
- Block design: odd replicas `ABBA`, even replicas `BAAB`; every pass uses a fresh process and store\n\
- Decision status: screening only; candidate regressions require confirmation before gating\n\n",
		escape_inline_code(&first.main_label),
		escape_inline_code(&first.pr_label),
		first.requests,
		first.clients,
		first.random_keyspace,
		expected_replicas,
		shards
			.iter()
			.map(|shard| shard.run_attempt)
			.collect::<BTreeSet<_>>()
			.into_iter()
			.map(|attempt| attempt.to_string())
			.collect::<Vec<_>>()
			.join(", ")
	);
	push_rps_table(
		&mut report,
		&groups,
		1,
		P1_RPS_MATERIALITY_PERCENT,
		P1_DUPLICATE_SPREAD_LIMIT_PERCENT,
	);
	push_rps_table(
		&mut report,
		&groups,
		first.pipeline_depth,
		PIPELINE_RPS_MATERIALITY_PERCENT,
		PIPELINE_DUPLICATE_SPREAD_LIMIT_PERCENT,
	);
	push_p1_latency_table(&mut report, &groups);
	report.push_str(
		"## Interpretation\n\n\
`candidate regression` means every stable screening replica fell below the negative materiality boundary; `candidate improvement` means every stable replica rose above the positive boundary. Both are triggers for confirmation, not failing or passing gates. `no material signal` means every observed effect stayed inside the inclusive materiality band; it does not establish equivalence. `mixed/inconclusive` means only some replicas crossed either boundary. `noisy` means a same-branch duplicate spread crossed its instability line or the cross-runner block effects were too dispersed for a useful conclusion. The initial materiality bands and instability lines are conservative heuristics and require A/A null calibration before any result can become a gate. Pipeline p50 remains in raw JSON but is intentionally omitted here because Redis 8 records pipelined batch/first-read latency rather than independent per-request latency.\n\n\
Raw pass output, deterministic seeds, server logs, and duplicate measurements are retained in the workflow artifacts.\n",
	);
	Ok(report)
}

fn escape_inline_code(value: &str) -> String {
	value.replace('`', "\\`").replace(['\n', '\r'], " ")
}

fn push_rps_table(
	report: &mut String,
	groups: &BTreeMap<(String, u64, u64), BTreeMap<u64, &BlockResult>>,
	pipeline_depth: u64,
	materiality_percent: f64,
	duplicate_spread_limit_percent: f64,
) {
	let effect_range_limit_pp = 2.0 * materiality_percent;
	report.push_str(&format!(
		"## P={pipeline_depth} RPS paired effects\n\nQuality vetoes, evaluated before materiality: same-branch duplicate spread `>{duplicate_spread_limit_percent:.0}%`, or cross-runner effect range width `>{effect_range_limit_pp:.0} pp`; either yields `noisy`. Screening materiality band: `±{materiality_percent:.0}%`; every stable replica below `-{materiality_percent:.0}%` is a candidate regression, and every stable replica above `+{materiality_percent:.0}%` is a candidate improvement.\n\n"
	));
	report.push_str(
		"| Command | Bytes | Blocks | Median Δ | MAD | Range | Max duplicate spread | Screening |\n\
|---|---:|---:|---:|---:|---:|---:|---|\n",
	);
	for ((command, data_size, depth), replicas) in groups {
		if *depth != pipeline_depth {
			continue;
		}
		let summary = summarize_metric(replicas.values().map(|block| &block.rps));
		report.push_str(&format!(
			"| {command} | {data_size} | {} | {:+.2}% | {:.2} pp | {:+.2}%..{:+.2}% | {:.2}% | {} |\n",
			replicas.len(),
			summary.median_delta,
			summary.mad,
			summary.min_delta,
			summary.max_delta,
			summary.max_duplicate_spread,
			screening_status(
				&summary,
				materiality_percent,
				duplicate_spread_limit_percent,
			),
		));
	}
	report.push('\n');
}

fn push_p1_latency_table(
	report: &mut String,
	groups: &BTreeMap<(String, u64, u64), BTreeMap<u64, &BlockResult>>,
) {
	report.push_str(
		"## P=1 p50 latency (informational)\n\n\
Latency remains descriptive while p95/p99 collection and null calibration are added.\n\n\
| Command | Bytes | p50 Blocks | Median Δ | Median PR-Main Δ | MAD | Range |\n\
|---|---:|---:|---:|---:|---:|---:|\n",
	);
	for ((command, data_size, depth), replicas) in groups {
		if *depth != 1 {
			continue;
		}
		let effects = replicas
			.values()
			.filter_map(|block| block.p50_msec.as_ref())
			.collect::<Vec<_>>();
		if effects.len() != replicas.len() {
			report.push_str(&format!(
				"| {command} | {data_size} | {}/{} | unavailable | unavailable | unavailable | unavailable |\n",
				effects.len(),
				replicas.len(),
			));
			continue;
		}
		let summary = summarize_metric(effects.iter().copied());
		report.push_str(&format!(
			"| {command} | {data_size} | {}/{} | {:+.2}% | {:+.3} ms | {:.2} pp | {:+.2}%..{:+.2}% |\n",
			effects.len(),
			replicas.len(),
			summary.median_delta,
			summary.median_absolute_delta,
			summary.mad,
			summary.min_delta,
			summary.max_delta,
		));
	}
	report.push('\n');
}

#[derive(Debug, PartialEq)]
struct MetricSummary {
	median_delta: f64,
	median_absolute_delta: f64,
	mad: f64,
	min_delta: f64,
	max_delta: f64,
	max_duplicate_spread: f64,
}

fn summarize_metric<'a>(effects: impl Iterator<Item = &'a MetricEffect>) -> MetricSummary {
	let effects = effects.collect::<Vec<_>>();
	let deltas = effects
		.iter()
		.map(|effect| effect.delta_percent)
		.collect::<Vec<_>>();
	let absolute_deltas = effects
		.iter()
		.map(|effect| effect.absolute_delta)
		.collect::<Vec<_>>();
	let median_delta = median(&deltas);
	MetricSummary {
		median_delta,
		median_absolute_delta: median(&absolute_deltas),
		mad: median(
			&deltas
				.iter()
				.map(|value| (value - median_delta).abs())
				.collect::<Vec<_>>(),
		),
		min_delta: deltas.iter().copied().fold(f64::INFINITY, f64::min),
		max_delta: deltas.iter().copied().fold(f64::NEG_INFINITY, f64::max),
		max_duplicate_spread: effects
			.iter()
			.flat_map(|effect| {
				[
					effect.main_duplicate_spread_percent,
					effect.pr_duplicate_spread_percent,
				]
			})
			.fold(0.0, f64::max),
	}
}

fn median(values: &[f64]) -> f64 {
	let mut values = values.to_vec();
	values.sort_by(f64::total_cmp);
	let middle = values.len() / 2;
	if values.len().is_multiple_of(2) {
		(values[middle - 1] + values[middle]) / 2.0
	} else {
		values[middle]
	}
}

fn screening_status(
	summary: &MetricSummary,
	materiality_percent: f64,
	duplicate_spread_limit_percent: f64,
) -> &'static str {
	if summary.max_duplicate_spread > duplicate_spread_limit_percent
		|| summary.max_delta - summary.min_delta > 2.0 * materiality_percent
	{
		"noisy"
	} else if summary.max_delta < -materiality_percent {
		"candidate regression"
	} else if summary.min_delta > materiality_percent {
		"candidate improvement"
	} else if summary.min_delta >= -materiality_percent && summary.max_delta <= materiality_percent
	{
		"no material signal"
	} else {
		"mixed/inconclusive"
	}
}

#[cfg(test)]
mod tests {
	use tempfile::tempdir;

	use super::*;

	fn pass(position: usize, branch: Branch, rps: f64, p50_msec: f64) -> PassResult {
		PassResult {
			position,
			branch,
			rps,
			p50_msec: Some(p50_msec),
			artifact_dir: format!("pass-{position}"),
		}
	}

	fn shard(commands: &[&str], data_size: u64, replica: u64, run_attempt: u64) -> ShardResult {
		let seed_base = 1;
		let blocks = commands
			.iter()
			.flat_map(|command| {
				let parsed_command = comparison_command(command).unwrap();
				[1, 50].map(move |pipeline_depth| {
					let order = block_order(replica);
					let passes = order
						.into_iter()
						.enumerate()
						.map(|(index, branch)| {
							let (rps, p50_msec) = match branch {
								Branch::Main => (100.0, 1.0),
								Branch::Pr => (99.0, 1.01),
							};
							pass(index + 1, branch, rps, p50_msec)
						})
						.collect::<Vec<_>>();
					let rps = metric_effect(&passes, |pass| Some(pass.rps))
						.unwrap()
						.unwrap();
					let p50_msec = metric_effect(&passes, |pass| pass.p50_msec).unwrap();
					BlockResult {
						command: (*command).to_string(),
						pipeline_depth,
						order: if replica.is_multiple_of(2) {
							"BAAB".into()
						} else {
							"ABBA".into()
						},
						seed: derive_seed(
							seed_base,
							parsed_command,
							data_size,
							pipeline_depth,
							replica,
						),
						passes,
						rps,
						p50_msec,
					}
				})
			})
			.collect();
		ShardResult {
			schema_version: SCHEMA_VERSION,
			main_label: "main".into(),
			pr_label: "pr".into(),
			data_size,
			replica,
			run_attempt,
			commands: commands
				.iter()
				.map(|command| (*command).to_string())
				.collect(),
			requests: 10,
			clients: 1,
			random_keyspace: 10,
			pipeline_depth: 50,
			threads: None,
			runtime_threads: None,
			seed_requests: 10,
			seed_base,
			settle_millis: 0,
			blocks,
		}
	}

	#[test]
	fn replicas_alternate_balanced_block_order() {
		assert_eq!(
			block_order(1),
			[Branch::Main, Branch::Pr, Branch::Pr, Branch::Main]
		);
		assert_eq!(
			block_order(2),
			[Branch::Pr, Branch::Main, Branch::Main, Branch::Pr]
		);
	}

	#[test]
	fn metric_effect_uses_geometric_branch_means() {
		let passes = vec![
			pass(1, Branch::Main, 100.0, 1.0),
			pass(2, Branch::Pr, 90.0, 1.1),
			pass(3, Branch::Pr, 90.0, 1.1),
			pass(4, Branch::Main, 100.0, 1.0),
		];
		let effect = metric_effect(&passes, |pass| Some(pass.rps))
			.unwrap()
			.unwrap();

		assert!((effect.delta_percent - -10.0).abs() < 1e-9);
		assert!((effect.main_duplicate_spread_percent).abs() < 1e-9);
		assert!((effect.pr_duplicate_spread_percent).abs() < 1e-9);
	}

	#[test]
	fn derived_seeds_are_stable_and_cell_specific() {
		let first = derive_seed(1, ComparisonCommand::Get, 512, 1, 1);
		assert!((1..=redis_benchmark::MAX_REDIS_RANDOM_SEED).contains(&first));
		assert_eq!(first, derive_seed(1, ComparisonCommand::Get, 512, 1, 1));
		assert_ne!(first, derive_seed(1, ComparisonCommand::Set, 512, 1, 1));
		assert_ne!(first, derive_seed(1, ComparisonCommand::Get, 1024, 1, 1));
		assert_ne!(first, derive_seed(1, ComparisonCommand::Get, 512, 50, 1));

		let mut ci_seeds = BTreeSet::new();
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
			for data_size in [512, 1024] {
				for pipeline_depth in [1, 50] {
					for replica in 1..=3 {
						let seed =
							derive_seed(277_000, command, data_size, pipeline_depth, replica);
						assert!((1..=redis_benchmark::MAX_REDIS_RANDOM_SEED).contains(&seed));
						assert!(ci_seeds.insert(seed));
					}
				}
			}
		}
		assert_eq!(ci_seeds.len(), 120);
	}

	#[test]
	fn screening_requires_every_replica_to_cross_materiality() {
		let regression = MetricSummary {
			median_delta: -7.0,
			median_absolute_delta: 0.0,
			mad: 0.5,
			min_delta: -8.0,
			max_delta: -6.0,
			max_duplicate_spread: 1.0,
		};
		assert_eq!(
			screening_status(&regression, 5.0, 10.0),
			"candidate regression"
		);

		let mixed = MetricSummary {
			max_delta: 2.0,
			..regression
		};
		assert_eq!(screening_status(&mixed, 5.0, 10.0), "mixed/inconclusive");

		let inside_lines = MetricSummary {
			median_delta: -1.0,
			mad: 1.0,
			min_delta: -3.0,
			max_delta: 2.0,
			..regression
		};
		assert_eq!(
			screening_status(&inside_lines, 5.0, 10.0),
			"no material signal"
		);

		let unstable = MetricSummary {
			max_duplicate_spread: 10.1,
			..regression
		};
		assert_eq!(screening_status(&unstable, 5.0, 10.0), "noisy");

		let dispersed = MetricSummary {
			median_delta: -50.0,
			mad: 44.0,
			min_delta: -100.0,
			max_delta: -6.0,
			..regression
		};
		assert_eq!(screening_status(&dispersed, 5.0, 10.0), "noisy");
	}

	#[test]
	fn report_rejects_missing_replicas() {
		let directory = tempdir().unwrap();
		let input = directory.path().join("input");
		fs::create_dir_all(&input).unwrap();
		let commands = redis_benchmark::COMPARISON_PROFILE_COMMANDS.to_vec();
		let shard = shard(&commands, 512, 1, 1);
		fs::write(
			input.join("result.json"),
			serde_json::to_string(&shard).unwrap(),
		)
		.unwrap();

		let error = build_report(&read_shards(&input).unwrap(), 2, &[512]).unwrap_err();
		assert!(error.contains("Incomplete replicas"));
	}

	#[test]
	fn report_accepts_the_complete_ci_matrix() {
		let command_shards: &[&[&str]] = &[
			&["GET", "SET"],
			&["HGET", "HSET"],
			&["LPOP", "LPUSH"],
			&["SADD", "SREM"],
			&["ZADD", "ZREM"],
		];
		let mut shards = Vec::new();
		for data_size in [512, 1024] {
			for replica in 1..=3 {
				for commands in command_shards {
					shards.push(shard(commands, data_size, replica, 1));
				}
			}
		}

		let report = build_report(&shards, 3, &[512, 1024]).unwrap();
		assert!(report.contains("Replicas per cell: `3` independent runners"));
		assert!(report.contains("## P=1 RPS paired effects"));
		assert!(report.contains("cross-runner effect range width `>10 pp`"));
		assert!(report.contains("Screening materiality band: `±5%`"));
		assert!(report.contains("above `+5%` is a candidate improvement"));
		assert!(report.contains("## P=50 RPS paired effects"));
		assert!(report.contains("| GET | 512 | 3 |"));
		assert!(report.contains("| ZREM | 1024 | 3 |"));
	}

	#[test]
	fn shard_reader_prefers_the_newest_workflow_attempt() {
		let directory = tempdir().unwrap();
		let input = directory.path().join("input");
		for attempt in [1, 2] {
			let artifact = input.join(format!("attempt-{attempt}"));
			fs::create_dir_all(&artifact).unwrap();
			fs::write(
				artifact.join("result.json"),
				serde_json::to_string(&shard(&["GET", "SET"], 512, 1, attempt)).unwrap(),
			)
			.unwrap();
		}

		let shards = read_shards(&input).unwrap();
		assert_eq!(shards.len(), 1);
		assert_eq!(shards[0].run_attempt, 2);
	}

	#[test]
	fn shard_reader_rejects_tampered_passes_and_effects() {
		let directory = tempdir().unwrap();
		let input = directory.path().join("input");
		fs::create_dir_all(&input).unwrap();
		let mut invalid_sequence = shard(&["GET", "SET"], 512, 1, 1);
		invalid_sequence.blocks[0].passes[0].branch = Branch::Pr;
		fs::write(
			input.join("result.json"),
			serde_json::to_string(&invalid_sequence).unwrap(),
		)
		.unwrap();
		let error = read_shards(&input).unwrap_err();
		assert!(error.contains("invalid pass sequence"));

		let effect_input = directory.path().join("effect-input");
		fs::create_dir_all(&effect_input).unwrap();
		let mut invalid_effect = shard(&["GET", "SET"], 512, 1, 1);
		invalid_effect.blocks[0].rps.delta_percent = 42.0;
		fs::write(
			effect_input.join("result.json"),
			serde_json::to_string(&invalid_effect).unwrap(),
		)
		.unwrap();
		let error = read_shards(&effect_input).unwrap_err();
		assert!(error.contains("RPS effect inconsistent"));
	}
}
