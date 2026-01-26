//! ```cargo
//! [dependencies]
//! regex = "1"
//! clap = { version = "4", features = ["derive"] }
//! ```

use std::collections::HashMap;
use std::fs;

use clap::Parser;
use regex::Regex;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
	/// Main branch benchmark output file
	#[arg(long)]
	main: String,

	/// PR branch benchmark output file
	#[arg(long)]
	pr: String,

	/// Redis benchmark output file
	#[arg(long)]
	redis: Option<String>,

	/// Main branch pipeline benchmark output file
	#[arg(long)]
	main_pipeline: String,

	/// PR branch pipeline benchmark output file
	#[arg(long)]
	pr_pipeline: String,

	/// Redis pipeline benchmark output file
	#[arg(long)]
	redis_pipeline: Option<String>,
}

fn main() {
	let args = Args::parse();

	let (main_map, pr_map, redis_map) =
		read_and_parse_benchmarks(&args.main, &args.pr, args.redis.as_ref(), "");

	// Print default mode table
	print_comparison_table(
		"### Benchmark Comparison 🚀",
		&main_map,
		&pr_map,
		redis_map.as_ref(),
	);

	// Print pipeline mode table
	let (main_pipeline_map, pr_pipeline_map, redis_pipeline_map) = read_and_parse_benchmarks(
		&args.main_pipeline,
		&args.pr_pipeline,
		args.redis_pipeline.as_ref(),
		"pipeline",
	);

	println!();
	println!("---");
	println!();

	print_comparison_table(
		"### Pipeline Benchmark Comparison (-P 50) ⚡",
		&main_pipeline_map,
		&pr_pipeline_map,
		redis_pipeline_map.as_ref(),
	);

	println!();
	println!("*Comparison triggered by automated benchmark.*");
}

fn read_and_parse_benchmarks(
	main_file: &str,
	pr_file: &str,
	redis_file: Option<&String>,
	benchmark_type: &str,
) -> (
	HashMap<String, f64>,
	HashMap<String, f64>,
	Option<HashMap<String, f64>>,
) {
	let main_content = fs::read_to_string(main_file)
		.unwrap_or_else(|_| panic!("Failed to read main {} benchmark file", benchmark_type));
	let pr_content = fs::read_to_string(pr_file)
		.unwrap_or_else(|_| panic!("Failed to read pr {} benchmark file", benchmark_type));
	let redis_content = redis_file.map(|f| {
		fs::read_to_string(f)
			.unwrap_or_else(|_| panic!("Failed to read redis {} benchmark file", benchmark_type))
	});

	let main_map = parse_benchmark(&main_content);
	let pr_map = parse_benchmark(&pr_content);
	let redis_map = redis_content.as_ref().map(|c| parse_benchmark(c));

	(main_map, pr_map, redis_map)
}

fn print_comparison_table(
	title: &str,
	main_map: &HashMap<String, f64>,
	pr_map: &HashMap<String, f64>,
	redis_map: Option<&HashMap<String, f64>>,
) {
	println!("{}", title);
	println!();

	if redis_map.is_some() {
		println!("| Command | PR RPS | Main RPS | Redis RPS | vs Main | vs Redis |");
		println!("|---|---|---|---|---|---|");
	} else {
		println!("| Command | PR RPS | Main RPS | vs Main |");
		println!("|---|---|---|---|");
	}

	// Collect all commands
	let mut commands: std::collections::BTreeSet<_> = main_map.keys().collect();
	commands.extend(pr_map.keys());
	if let Some(r_map) = redis_map {
		commands.extend(r_map.keys());
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

		if let Some(r_map) = redis_map {
			let redis_rps = r_map.get(cmd).copied().unwrap_or(0.0);

			// Calculate vs Redis diff
			let redis_diff_percent = if redis_rps > 0.0 {
				((pr_rps - redis_rps) / redis_rps) * 100.0
			} else if pr_rps > 0.0 {
				100.0
			} else {
				0.0
			};

			let redis_icon = if redis_diff_percent > 0.0 {
				"🏆 "
			} else {
				""
			};
			let vs_redis_cell = if redis_rps > 0.0 {
				format!("{}{:+.2}%", redis_icon, redis_diff_percent)
			} else {
				"-".to_string()
			};

			println!(
				"| {} | {:.2} | {:.2} | {:.2} | {} | {} |",
				cmd, pr_rps, main_rps, redis_rps, vs_main_cell, vs_redis_cell
			);
		} else {
			println!(
				"| {} | {:.2} | {:.2} | {} |",
				cmd, pr_rps, main_rps, vs_main_cell
			);
		}
	}
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
