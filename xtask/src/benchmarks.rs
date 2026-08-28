use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs;

use clap::Args as ClapArgs;
use regex::Regex;

use crate::write_stdout;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BenchmarkResult {
	pub(crate) rps: f64,
	pub(crate) p50_msec: Option<f64>,
}

type BenchmarkMap = HashMap<String, BenchmarkResult>;
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

	/// Label used for the main/base result columns.
	#[arg(long, default_value = "Main")]
	pub main_label: String,

	/// Label used for the PR/head result columns.
	#[arg(long, default_value = "PR")]
	pub pr_label: String,

	/// Pipeline depth represented by the pipeline result files.
	#[arg(long, default_value_t = 50)]
	pub pipeline_depth: u64,
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
		&args.main_label,
		&args.pr_label,
	);
	push_latency_table(
		&mut report,
		&main_map,
		&pr_map,
		&baselines,
		&args.main_label,
		&args.pr_label,
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
		&format!(
			"### Pipeline Benchmark Comparison (-P {}) ⚡",
			args.pipeline_depth
		),
		&main_pipeline_map,
		&pr_pipeline_map,
		&baseline_pipelines,
		&args.main_label,
		&args.pr_label,
	);
	push_latency_table(
		&mut report,
		&main_pipeline_map,
		&pr_pipeline_map,
		&baseline_pipelines,
		&args.main_label,
		&args.pr_label,
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
	validate_comparison_maps(&main_map, &pr_map, benchmark_type)?;
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
	main_label: &str,
	pr_label: &str,
) {
	report.push_str(title);
	report.push_str("\n\n");

	let mut headers = vec![
		"Command".to_string(),
		format!("{} RPS", sanitize_markdown_table_text(pr_label)),
		format!("{} RPS", sanitize_markdown_table_text(main_label)),
	];
	for (name, _) in baselines {
		headers.push(format!("{} RPS", sanitize_markdown_table_text(name)));
	}
	headers.push(format!("vs {}", sanitize_markdown_table_text(main_label)));
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
		let main_rps = main_map.get(cmd).map(|result| result.rps).unwrap_or(0.0);
		let pr_rps = pr_map.get(cmd).map(|result| result.rps).unwrap_or(0.0);

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
			let baseline_rps = baseline_map
				.get(cmd)
				.map(|result| result.rps)
				.unwrap_or(0.0);
			row.push(format!("{baseline_rps:.2}"));
		}
		row.push(vs_main_cell);

		for (_, baseline_map) in baselines {
			let baseline_rps = baseline_map
				.get(cmd)
				.map(|result| result.rps)
				.unwrap_or(0.0);
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

fn push_latency_table(
	report: &mut String,
	main_map: &BenchmarkMap,
	pr_map: &BenchmarkMap,
	baselines: &[NamedBenchmarkMap],
	main_label: &str,
	pr_label: &str,
) {
	let has_latency = main_map
		.values()
		.chain(pr_map.values())
		.chain(baselines.iter().flat_map(|(_, map)| map.values()))
		.any(|result| result.p50_msec.is_some());
	if !has_latency {
		return;
	}

	report.push_str("\n#### p50 Latency (ms, lower is better)\n\n");
	let mut headers = vec![
		"Command".to_string(),
		format!("{} p50", sanitize_markdown_table_text(pr_label)),
		format!("{} p50", sanitize_markdown_table_text(main_label)),
	];
	for (name, _) in baselines {
		headers.push(format!("{} p50", sanitize_markdown_table_text(name)));
	}
	headers.push(format!("vs {}", sanitize_markdown_table_text(main_label)));
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
		let main_p50 = main_map.get(cmd).and_then(|result| result.p50_msec);
		let pr_p50 = pr_map.get(cmd).and_then(|result| result.p50_msec);
		let mut row = vec![
			cmd.to_string(),
			format_latency(pr_p50),
			format_latency(main_p50),
		];
		for (_, baseline_map) in baselines {
			row.push(format_latency(
				baseline_map.get(cmd).and_then(|result| result.p50_msec),
			));
		}
		row.push(format_latency_difference(pr_p50, main_p50, false));
		for (_, baseline_map) in baselines {
			row.push(format_latency_difference(
				pr_p50,
				baseline_map.get(cmd).and_then(|result| result.p50_msec),
				true,
			));
		}

		report.push_str(&format!("| {} |\n", row.join(" | ")));
	}
}

fn format_latency(latency: Option<f64>) -> String {
	latency
		.map(|latency| format!("{latency:.3}"))
		.unwrap_or_else(|| "-".to_string())
}

fn format_latency_difference(
	candidate: Option<f64>,
	reference: Option<f64>,
	trophy: bool,
) -> String {
	let (Some(candidate), Some(reference)) = (candidate, reference) else {
		return "-".to_string();
	};
	if reference <= 0.0 {
		return "-".to_string();
	}

	let difference = ((candidate - reference) / reference) * 100.0;
	let icon = if trophy && difference < 0.0 {
		"🏆 "
	} else if difference < -5.0 {
		"✅ "
	} else if difference > 5.0 {
		"⚠️ "
	} else {
		""
	};
	format!("{}{:+.2}%", icon, difference)
}

fn validate_comparison_maps(
	main_map: &BenchmarkMap,
	pr_map: &BenchmarkMap,
	benchmark_type: &str,
) -> Result<(), String> {
	let type_label = if benchmark_type.is_empty() {
		"standard"
	} else {
		benchmark_type
	};
	if main_map.is_empty() {
		return Err(format!(
			"No parseable commands found in main {type_label} benchmark output"
		));
	}
	if pr_map.is_empty() {
		return Err(format!(
			"No parseable commands found in pr {type_label} benchmark output"
		));
	}

	let main_commands = main_map.keys().collect::<BTreeSet<_>>();
	let pr_commands = pr_map.keys().collect::<BTreeSet<_>>();
	if main_commands != pr_commands {
		let missing_from_pr = main_commands
			.difference(&pr_commands)
			.map(|value| value.as_str())
			.collect::<Vec<_>>();
		let missing_from_main = pr_commands
			.difference(&main_commands)
			.map(|value| value.as_str())
			.collect::<Vec<_>>();
		return Err(format!(
			"Mismatched {type_label} benchmark commands; missing from pr: [{}]; missing from main: [{}]",
			missing_from_pr.join(", "),
			missing_from_main.join(", ")
		));
	}
	Ok(())
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

pub(crate) fn parse_benchmark(content: &str) -> HashMap<String, BenchmarkResult> {
	let mut map = HashMap::new();
	let rps_re =
		Regex::new(r"^([[:alnum:]_-]+)\b.*?:\s+(\d+(?:\.\d+)?)\s+requests per second(?:,|$)")
			.unwrap();
	let p50_re = Regex::new(r"(?:^|,\s*)p50=(\d+(?:\.\d+)?)\s+msec(?:,|$)").unwrap();

	for line in content.split(['\n', '\r']).map(str::trim) {
		if let Some(caps) = rps_re.captures(line) {
			let cmd = caps.get(1).unwrap().as_str();
			let rps_str = caps.get(2).unwrap().as_str();
			if let Ok(rps) = rps_str.parse::<f64>() {
				let p50_msec = p50_re
					.captures(line)
					.and_then(|captures| captures.get(1))
					.and_then(|value| value.as_str().parse::<f64>().ok());
				map.insert(cmd.to_string(), BenchmarkResult { rps, p50_msec });
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
		let dir = tempfile::tempdir().unwrap();
		let main = dir.path().join("main.txt");
		let pr = dir.path().join("pr.txt");
		let baseline = dir.path().join("redis.txt");
		let main_pipeline = dir.path().join("main_pipeline.txt");
		let pr_pipeline = dir.path().join("pr_pipeline.txt");
		let baseline_pipeline = dir.path().join("redis_pipeline.txt");

		std::fs::write(&main, "SET: 100.00 requests per second, p50=0.100 msec\n").unwrap();
		std::fs::write(&pr, "SET: 110.00 requests per second, p50=0.080 msec\n").unwrap();
		std::fs::write(
			&baseline,
			"SET: 90.00 requests per second, p50=0.120 msec\n",
		)
		.unwrap();
		std::fs::write(
			&main_pipeline,
			"GET: 200.00 requests per second, p50=0.200 msec\n",
		)
		.unwrap();
		std::fs::write(
			&pr_pipeline,
			"GET: 190.00 requests per second, p50=0.300 msec\n",
		)
		.unwrap();
		std::fs::write(
			&baseline_pipeline,
			"GET: 180.00 requests per second, p50=0.250 msec\n",
		)
		.unwrap();

		let args = Args {
			main: main.display().to_string(),
			pr: pr.display().to_string(),
			baselines: vec![format!("Redis={}", baseline.display())],
			main_pipeline: main_pipeline.display().to_string(),
			pr_pipeline: pr_pipeline.display().to_string(),
			baseline_pipelines: vec![format!("Redis={}", baseline_pipeline.display())],
			main_label: "Main".into(),
			pr_label: "PR".into(),
			pipeline_depth: 50,
		};

		let report = build_report(&args).unwrap();

		assert!(report.contains("### Benchmark Comparison 🚀"));
		assert!(report.contains("### Pipeline Benchmark Comparison (-P 50) ⚡"));
		assert!(report.contains("| SET | 110.00 | 100.00 | 90.00 | ✅ +10.00% | 🏆 +22.22% |"));
		assert!(report.contains("#### p50 Latency (ms, lower is better)"));
		assert!(report.contains("| SET | 0.080 | 0.100 | 0.120 | ✅ -20.00% | 🏆 -33.33% |"));
		assert!(report.contains("| GET | 190.00 | 200.00 | 180.00 | -5.00% | 🏆 +5.56% |"));
		assert!(report.contains("| GET | 0.300 | 0.200 | 0.250 | ⚠️ +50.00% | ⚠️ +20.00% |"));
	}

	#[test]
	fn report_uses_custom_labels_and_pipeline_depth() {
		let dir = tempfile::tempdir().unwrap();
		let main = dir.path().join("main.txt");
		let pr = dir.path().join("pr.txt");
		let main_pipeline = dir.path().join("main_pipeline.txt");
		let pr_pipeline = dir.path().join("pr_pipeline.txt");
		std::fs::write(&main, "SET: 100.00 requests per second\n").unwrap();
		std::fs::write(&pr, "SET: 110.00 requests per second\n").unwrap();
		std::fs::write(&main_pipeline, "SET: 200.00 requests per second\n").unwrap();
		std::fs::write(&pr_pipeline, "SET: 220.00 requests per second\n").unwrap();

		let report = build_report(&Args {
			main: main.display().to_string(),
			pr: pr.display().to_string(),
			baselines: Vec::new(),
			main_pipeline: main_pipeline.display().to_string(),
			pr_pipeline: pr_pipeline.display().to_string(),
			baseline_pipelines: Vec::new(),
			main_label: "main".into(),
			pr_label: "feature|fast".into(),
			pipeline_depth: 16,
		})
		.unwrap();

		assert!(report.contains("feature\\|fast RPS"));
		assert!(report.contains("main RPS"));
		assert!(report.contains("Pipeline Benchmark Comparison (-P 16)"));
		assert!(!report.contains("p50 Latency"));
	}

	#[test]
	fn report_rejects_mismatched_command_sets() {
		let dir = tempfile::tempdir().unwrap();
		let main = dir.path().join("main.txt");
		let pr = dir.path().join("pr.txt");
		std::fs::write(&main, "SET: 100.00 requests per second\n").unwrap();
		std::fs::write(&pr, "GET: 110.00 requests per second\n").unwrap();

		let error = read_and_parse_benchmarks(
			&main.display().to_string(),
			&pr.display().to_string(),
			&[],
			"",
		)
		.unwrap_err();

		assert!(error.contains("Mismatched standard benchmark commands"));
		assert!(error.contains("missing from pr: [SET]"));
		assert!(error.contains("missing from main: [GET]"));
	}

	#[test]
	fn parse_benchmark_uses_command_token_for_custom_commands() {
		let content = "HGET bench:hash field1: 123.45 requests per second, p50=0.095 msec\n";
		let parsed = parse_benchmark(content);
		let result = parsed.get("HGET").unwrap();

		assert_eq!(result.rps, 123.45);
		assert_eq!(result.p50_msec, Some(0.095));
		assert!(!parsed.contains_key("field1"));
	}

	#[test]
	fn parse_benchmark_keeps_rps_when_latency_is_missing_or_invalid() {
		let content = concat!(
			"SET: 100.00 requests per second\n",
			"GET: 90.00 requests per second, p50=unavailable msec\n",
		);
		let parsed = parse_benchmark(content);

		assert_eq!(parsed.get("SET").unwrap().rps, 100.0);
		assert_eq!(parsed.get("SET").unwrap().p50_msec, None);
		assert_eq!(parsed.get("GET").unwrap().rps, 90.0);
		assert_eq!(parsed.get("GET").unwrap().p50_msec, None);
	}

	#[test]
	fn parse_benchmark_ignores_carriage_return_progress_updates() {
		let content = concat!(
			"SET: rps=0.0 (overall: 0.0) avg_msec=0.000\r",
			"SET: 10000.00 requests per second, p50=0.063 msec\n",
			"GET: rps=0.0 (overall: 0.0) avg_msec=0.000\r",
			"GET: 9000.00 requests per second, p50=0.039 msec\n",
		);

		let parsed = parse_benchmark(content);

		assert_eq!(parsed.get("SET").unwrap().rps, 10000.0);
		assert_eq!(parsed.get("SET").unwrap().p50_msec, Some(0.063));
		assert_eq!(parsed.get("GET").unwrap().rps, 9000.0);
		assert_eq!(parsed.get("GET").unwrap().p50_msec, Some(0.039));
		assert_eq!(parsed.len(), 2);
	}
}
