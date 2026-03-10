//! ```cargo
//! [dependencies]
//! regex = "1"
//! clap = { version = "4", features = ["derive"] }
//! ```
//!
//! This script compares benchmark results between two branches (typically Main
//! and PR) and optionally against one or more baselines. It generates a
//! Markdown table summarizing the requests per second (RPS) and the percentage
//! difference.
//!
//! Usage:
//!     rust-script scripts/compare_benchmarks.rs \
//!         --main <main_bench_file> \
//!         --pr <pr_bench_file> \
//!         --main-pipeline <main_pipeline_file> \
//!         --pr-pipeline <pr_pipeline_file> \
//!         [--baseline <name=bench_file>]... \
//!         [--baseline-pipeline <name=bench_file>]...

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs;

use clap::Parser;
use regex::Regex;

type BenchmarkMap = HashMap<String, f64>;
type NamedBenchmarkMap = (String, BenchmarkMap);

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
	/// Main branch benchmark output file
	#[arg(long)]
	main: String,

	/// PR branch benchmark output file
	#[arg(long)]
	pr: String,

	/// Baseline benchmark output file in the form <name=path>
	#[arg(long = "baseline", value_name = "NAME=PATH")]
	baselines: Vec<String>,

	/// Main branch pipeline benchmark output file
	#[arg(long)]
	main_pipeline: String,

	/// PR branch pipeline benchmark output file
	#[arg(long)]
	pr_pipeline: String,

	/// Baseline pipeline benchmark output file in the form <name=path>
	#[arg(long = "baseline-pipeline", value_name = "NAME=PATH")]
	baseline_pipelines: Vec<String>,
}

fn main() {
	let args = Args::parse();

	let (main_map, pr_map, baselines) =
		read_and_parse_benchmarks(&args.main, &args.pr, &args.baselines, "");

	// Print default mode table
	print_comparison_table(
		"### Benchmark Comparison 🚀",
		&main_map,
		&pr_map,
		&baselines,
	);

	println!();
	println!("---");
	println!();

	let (main_pipeline_map, pr_pipeline_map, baseline_pipelines) = read_and_parse_benchmarks(
		&args.main_pipeline,
		&args.pr_pipeline,
		&args.baseline_pipelines,
		"pipeline",
	);

	print_comparison_table(
		"### Pipeline Benchmark Comparison (-P 50) ⚡",
		&main_pipeline_map,
		&pr_pipeline_map,
		&baseline_pipelines,
	);

	println!();
	println!("*Comparison triggered by automated benchmark.*");
}

fn read_and_parse_benchmarks(
	main_file: &str,
	pr_file: &str,
	baseline_files: &[String],
	benchmark_type: &str,
) -> (BenchmarkMap, BenchmarkMap, Vec<NamedBenchmarkMap>) {
	let main_content = fs::read_to_string(main_file)
		.unwrap_or_else(|_| panic!("Failed to read main {} benchmark file", benchmark_type));
	let pr_content = fs::read_to_string(pr_file)
		.unwrap_or_else(|_| panic!("Failed to read pr {} benchmark file", benchmark_type));

	let main_map = parse_benchmark(&main_content);
	let pr_map = parse_benchmark(&pr_content);
	let baselines = baseline_files
		.iter()
		.map(|entry| {
			let (name, path) = parse_named_path(entry, benchmark_type);
			let content = fs::read_to_string(&path).unwrap_or_else(|_| {
				panic!("Failed to read {} {} benchmark file", name, benchmark_type)
			});
			(name, parse_benchmark(&content))
		})
		.collect();

	(main_map, pr_map, baselines)
}

fn print_comparison_table(
	title: &str,
	main_map: &BenchmarkMap,
	pr_map: &BenchmarkMap,
	baselines: &[NamedBenchmarkMap],
) {
	println!("{}", title);
	println!();

	let mut headers = vec![
		"Command".to_string(),
		"PR RPS".to_string(),
		"Main RPS".to_string(),
	];
	for (name, _) in baselines {
		headers.push(format!("{} RPS", name));
	}
	headers.push("vs Main".to_string());
	for (name, _) in baselines {
		headers.push(format!("vs {}", name));
	}
	println!("| {} |", headers.join(" | "));
	println!("|{}|", vec!["---"; headers.len()].join("|"));

	// Collect all commands
	let mut commands: BTreeSet<_> = main_map.keys().collect();
	commands.extend(pr_map.keys());
	for (_, baseline_map) in baselines {
		commands.extend(baseline_map.keys());
	}

	for cmd in commands {
		let main_rps = main_map.get(cmd).copied().unwrap_or(0.0);
		let pr_rps = pr_map.get(cmd).copied().unwrap_or(0.0);

		// Calculate vs Main diff
		let pr_diff_percent = if main_rps > 0.0 {
			((pr_rps - main_rps) / main_rps) * 100.0
		} else if pr_rps > 0.0 {
			100.0
		} else {
			0.0
		};

		let pr_icon = if pr_diff_percent > 5.0 {
			"✅ "
		} else if pr_diff_percent < -5.0 {
			"⚠️ "
		} else {
			""
		};
		let vs_main_cell = if main_rps > 0.0 {
			format!("{}{:+.2}%", pr_icon, pr_diff_percent)
		} else {
			"-".to_string()
		};

		let mut row = vec![
			cmd.to_string(),
			format!("{:.2}", pr_rps),
			format!("{:.2}", main_rps),
		];
		for (_, baseline_map) in baselines {
			let baseline_rps = baseline_map.get(cmd).copied().unwrap_or(0.0);
			row.push(format!("{:.2}", baseline_rps));
		}
		row.push(vs_main_cell);

		for (_, baseline_map) in baselines {
			let baseline_rps = baseline_map.get(cmd).copied().unwrap_or(0.0);
			let baseline_diff_percent = if baseline_rps > 0.0 {
				((pr_rps - baseline_rps) / baseline_rps) * 100.0
			} else if pr_rps > 0.0 {
				100.0
			} else {
				0.0
			};

			let baseline_icon = if baseline_diff_percent > 0.0 {
				"🏆 "
			} else {
				""
			};
			let baseline_cell = if baseline_rps > 0.0 {
				format!("{}{:+.2}%", baseline_icon, baseline_diff_percent)
			} else {
				"-".to_string()
			};
			row.push(baseline_cell);
		}

		println!("| {} |", row.join(" | "));
	}
}

fn parse_named_path(value: &str, benchmark_type: &str) -> (String, String) {
	let (name, path) = value.split_once('=').unwrap_or_else(|| {
		panic!(
			"Invalid {} baseline argument '{}', expected NAME=PATH",
			benchmark_type, value
		)
	});

	let trimmed_name = name.trim();
	let trimmed_path = path.trim();
	if trimmed_name.is_empty() || trimmed_path.is_empty() {
		panic!(
			"Invalid {} baseline argument '{}', expected non-empty NAME and PATH",
			benchmark_type, value
		);
	}

	(trimmed_name.to_string(), trimmed_path.to_string())
}

fn parse_benchmark(content: &str) -> HashMap<String, f64> {
	let mut map = HashMap::new();
	// Regex to match "SET: 12345.67 requests per second"
	// Adjust regex if format varies, but usually `redis-benchmark -q` outputs
	// straightforward lines. Sometimes it might strip the colon or be key="value".
	// Standard `redis-benchmark -q` output:
	// PING_INLINE: 106382.98 requests per second
	// SET: 104166.66 requests per second
	let re = Regex::new(r"([\w_]+):\s+([\d\.]+)\s+requests per second").unwrap();

	for line in content.lines() {
		if let Some(caps) = re.captures(line) {
			let cmd = caps.get(1).unwrap().as_str();
			let rps_str = caps.get(2).unwrap().as_str();
			if let Ok(rps) = rps_str.parse::<f64>() {
				map.insert(cmd.to_string(), rps);
			}
		}
	}
	map
}
