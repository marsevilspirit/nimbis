use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs;

use clap::Args as ClapArgs;
use regex::Regex;

use crate::write_stdout;

type BenchmarkMap = HashMap<String, f64>;
type NamedBenchmarkMap = (String, BenchmarkMap);

#[derive(ClapArgs, Debug)]
pub struct Args {
	/// Main branch benchmark output file
	#[arg(long)]
	pub main: String,

	/// PR branch benchmark output file
	#[arg(long)]
	pub pr: String,

	/// Baseline benchmark output file in the form <name=path>
	#[arg(long = "baseline", value_name = "NAME=PATH")]
	pub baselines: Vec<String>,

	/// Main branch pipeline benchmark output file
	#[arg(long)]
	pub main_pipeline: String,

	/// PR branch pipeline benchmark output file
	#[arg(long)]
	pub pr_pipeline: String,

	/// Baseline pipeline benchmark output file in the form <name=path>
	#[arg(long = "baseline-pipeline", value_name = "NAME=PATH")]
	pub baseline_pipelines: Vec<String>,
}

pub fn compare_benchmarks(args: Args) -> Result<(), String> {
	let report = build_report(&args)?;
	write_stdout(&report)?;
	Ok(())
}

pub fn build_report(args: &Args) -> Result<String, String> {
	let (main_map, pr_map, baselines) =
		read_and_parse_benchmarks(&args.main, &args.pr, &args.baselines, "")?;

	let mut report = String::new();
	push_comparison_table(
		&mut report,
		"### Benchmark Comparison 🚀",
		&main_map,
		&pr_map,
		&baselines,
	);
	report.push('\n');
	report.push_str("---\n\n");

	let (main_pipeline_map, pr_pipeline_map, baseline_pipelines) = read_and_parse_benchmarks(
		&args.main_pipeline,
		&args.pr_pipeline,
		&args.baseline_pipelines,
		"pipeline",
	)?;

	push_comparison_table(
		&mut report,
		"### Pipeline Benchmark Comparison (-P 50) ⚡",
		&main_pipeline_map,
		&pr_pipeline_map,
		&baseline_pipelines,
	);
	report.push('\n');
	report.push_str("*Comparison triggered by automated benchmark.*\n");

	Ok(report)
}

fn read_and_parse_benchmarks(
	main_file: &str,
	pr_file: &str,
	baseline_files: &[String],
	benchmark_type: &str,
) -> Result<(BenchmarkMap, BenchmarkMap, Vec<NamedBenchmarkMap>), String> {
	let main_content = fs::read_to_string(main_file)
		.map_err(|_| format!("Failed to read main {benchmark_type} benchmark file"))?;
	let pr_content = fs::read_to_string(pr_file)
		.map_err(|_| format!("Failed to read pr {benchmark_type} benchmark file"))?;

	let main_map = parse_benchmark(&main_content);
	let pr_map = parse_benchmark(&pr_content);
	let baselines = baseline_files
		.iter()
		.map(|entry| {
			let (name, path) = parse_named_path(entry, benchmark_type)?;
			let content = fs::read_to_string(&path)
				.map_err(|_| format!("Failed to read {name} {benchmark_type} benchmark file"))?;
			Ok((name, parse_benchmark(&content)))
		})
		.collect::<Result<_, String>>()?;

	Ok((main_map, pr_map, baselines))
}

fn push_comparison_table(
	report: &mut String,
	title: &str,
	main_map: &BenchmarkMap,
	pr_map: &BenchmarkMap,
	baselines: &[NamedBenchmarkMap],
) {
	report.push_str(title);
	report.push_str("\n\n");

	let mut headers = vec![
		"Command".to_string(),
		"PR RPS".to_string(),
		"Main RPS".to_string(),
	];
	for (name, _) in baselines {
		headers.push(format!("{} RPS", sanitize_markdown_table_text(name)));
	}
	headers.push("vs Main".to_string());
	for (name, _) in baselines {
		headers.push(format!("vs {}", sanitize_markdown_table_text(name)));
	}
	report.push_str(&format!("| {} |\n", headers.join(" | ")));
	report.push_str(&format!("|{}|\n", vec!["---"; headers.len()].join("|")));

	let mut commands: BTreeSet<_> = main_map.keys().collect();
	commands.extend(pr_map.keys());
	for (_, baseline_map) in baselines {
		commands.extend(baseline_map.keys());
	}

	for cmd in commands {
		let main_rps = main_map.get(cmd).copied().unwrap_or(0.0);
		let pr_rps = pr_map.get(cmd).copied().unwrap_or(0.0);

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
			format!("{pr_rps:.2}"),
			format!("{main_rps:.2}"),
		];
		for (_, baseline_map) in baselines {
			let baseline_rps = baseline_map.get(cmd).copied().unwrap_or(0.0);
			row.push(format!("{baseline_rps:.2}"));
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

		report.push_str(&format!("| {} |\n", row.join(" | ")));
	}
}

fn sanitize_markdown_table_text(value: &str) -> String {
	value
		.lines()
		.map(str::trim)
		.filter(|part| !part.is_empty())
		.collect::<Vec<_>>()
		.join(" ")
		.replace('|', "\\|")
}

fn parse_named_path(value: &str, benchmark_type: &str) -> Result<(String, String), String> {
	let (name, path) = value.split_once('=').ok_or_else(|| {
		format!("Invalid {benchmark_type} baseline argument '{value}', expected NAME=PATH")
	})?;

	let trimmed_name = name.trim();
	let trimmed_path = path.trim();
	if trimmed_name.is_empty() || trimmed_path.is_empty() {
		return Err(format!(
			"Invalid {benchmark_type} baseline argument '{value}', expected non-empty NAME and PATH"
		));
	}

	Ok((trimmed_name.to_string(), trimmed_path.to_string()))
}

fn parse_benchmark(content: &str) -> HashMap<String, f64> {
	let mut map = HashMap::new();
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn report_contains_default_and_pipeline_tables() {
		let dir = std::env::temp_dir().join(format!("nimbis-xtask-bench-{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let main = dir.join("main.txt");
		let pr = dir.join("pr.txt");
		let baseline = dir.join("redis.txt");
		let main_pipeline = dir.join("main_pipeline.txt");
		let pr_pipeline = dir.join("pr_pipeline.txt");
		let baseline_pipeline = dir.join("redis_pipeline.txt");

		std::fs::write(&main, "SET: 100.00 requests per second\n").unwrap();
		std::fs::write(&pr, "SET: 110.00 requests per second\n").unwrap();
		std::fs::write(&baseline, "SET: 90.00 requests per second\n").unwrap();
		std::fs::write(&main_pipeline, "GET: 200.00 requests per second\n").unwrap();
		std::fs::write(&pr_pipeline, "GET: 190.00 requests per second\n").unwrap();
		std::fs::write(&baseline_pipeline, "GET: 180.00 requests per second\n").unwrap();

		let args = Args {
			main: main.display().to_string(),
			pr: pr.display().to_string(),
			baselines: vec![format!("Redis={}", baseline.display())],
			main_pipeline: main_pipeline.display().to_string(),
			pr_pipeline: pr_pipeline.display().to_string(),
			baseline_pipelines: vec![format!("Redis={}", baseline_pipeline.display())],
		};

		let report = build_report(&args).unwrap();

		assert!(report.contains("### Benchmark Comparison 🚀"));
		assert!(report.contains("### Pipeline Benchmark Comparison (-P 50) ⚡"));
		assert!(report.contains("| SET | 110.00 | 100.00 | 90.00 | ✅ +10.00% | 🏆 +22.22% |"));
		assert!(report.contains("| GET | 190.00 | 200.00 | 180.00 | -5.00% | 🏆 +5.56% |"));

		std::fs::remove_dir_all(dir).unwrap();
	}
}
