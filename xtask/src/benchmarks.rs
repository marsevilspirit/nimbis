use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs;

use clap::Args as ClapArgs;
use regex::Regex;

use crate::write_stdout;

type BenchmarkMap = HashMap<String, f64>;
type NamedBenchmarkMaps = (String, Vec<BenchmarkMap>);
type ParsedBenchmarks = (
	Vec<BenchmarkMap>,
	Vec<BenchmarkMap>,
	Vec<NamedBenchmarkMaps>,
);

#[derive(ClapArgs, Debug)]
pub struct Args {
	/// Main branch benchmark output file. Repeat for paired samples.
	#[arg(long, required = true)]
	pub main: Vec<String>,

	/// PR branch benchmark output file. Repeat in the same order as --main.
	#[arg(long, required = true)]
	pub pr: Vec<String>,

	/// Baseline benchmark output file in the form <name=path>
	#[arg(long = "baseline", value_name = "NAME=PATH")]
	pub baselines: Vec<String>,

	/// Main branch pipeline benchmark output file. Repeat for paired samples.
	#[arg(long, required = true)]
	pub main_pipeline: Vec<String>,

	/// PR branch pipeline benchmark output file. Repeat in the same order as
	/// --main-pipeline.
	#[arg(long, required = true)]
	pub pr_pipeline: Vec<String>,

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
	main_files: &[String],
	pr_files: &[String],
	baseline_files: &[String],
	benchmark_type: &str,
) -> Result<ParsedBenchmarks, String> {
	let description = benchmark_description(benchmark_type);
	if main_files.len() != pr_files.len() {
		return Err(format!(
			"Mismatched main and PR {description} sample counts: {} != {}",
			main_files.len(),
			pr_files.len()
		));
	}
	if main_files.is_empty() {
		return Err(format!("No paired {description} samples provided"));
	}

	let main_maps = read_benchmark_files(main_files, "main", benchmark_type)?;
	let pr_maps = read_benchmark_files(pr_files, "PR", benchmark_type)?;
	validate_paired_samples(&main_maps, &pr_maps, &description)?;
	let mut baselines = BTreeMap::<String, Vec<BenchmarkMap>>::new();
	for entry in baseline_files {
		let (name, path) = parse_named_path(entry, benchmark_type)?;
		let content = fs::read_to_string(&path)
			.map_err(|_| format!("Failed to read {name} {description} file"))?;
		baselines
			.entry(name)
			.or_default()
			.push(parse_benchmark(&content));
	}

	Ok((main_maps, pr_maps, baselines.into_iter().collect()))
}

fn validate_paired_samples(
	main_maps: &[BenchmarkMap],
	pr_maps: &[BenchmarkMap],
	description: &str,
) -> Result<(), String> {
	let mut expected_commands = None;

	for (index, (main_map, pr_map)) in main_maps.iter().zip(pr_maps).enumerate() {
		let pair_number = index + 1;
		let main_commands = main_map.keys().cloned().collect::<BTreeSet<_>>();
		let pr_commands = pr_map.keys().cloned().collect::<BTreeSet<_>>();
		if main_commands.is_empty() || pr_commands.is_empty() {
			return Err(format!(
				"Paired {description} sample {pair_number} contains no parsed commands"
			));
		}
		if main_commands != pr_commands {
			let only_main = main_commands
				.difference(&pr_commands)
				.cloned()
				.collect::<Vec<_>>()
				.join(", ");
			let only_pr = pr_commands
				.difference(&main_commands)
				.cloned()
				.collect::<Vec<_>>()
				.join(", ");
			return Err(format!(
				"Paired {description} sample {pair_number} command mismatch (only Main: [{only_main}]; only PR: [{only_pr}])"
			));
		}
		for (side, map) in [("Main", main_map), ("PR", pr_map)] {
			if let Some((command, value)) = map.iter().find(|(_, value)| **value <= 0.0) {
				return Err(format!(
					"Paired {description} sample {pair_number} has non-positive {side} RPS for {command}: {value}"
				));
			}
		}

		if let Some(expected) = &expected_commands
			&& &main_commands != expected
		{
			return Err(format!(
				"Paired {description} sample {pair_number} commands differ from sample 1"
			));
		}
		expected_commands.get_or_insert(main_commands);
	}

	Ok(())
}

fn read_benchmark_files(
	files: &[String],
	label: &str,
	benchmark_type: &str,
) -> Result<Vec<BenchmarkMap>, String> {
	let description = benchmark_description(benchmark_type);
	files
		.iter()
		.map(|path| {
			let content = fs::read_to_string(path).map_err(|error| {
				format!("Failed to read {label} {description} file '{path}': {error}")
			})?;
			Ok(parse_benchmark(&content))
		})
		.collect()
}

fn benchmark_description(benchmark_type: &str) -> String {
	if benchmark_type.is_empty() {
		"benchmark".to_string()
	} else {
		format!("{benchmark_type} benchmark")
	}
}

fn push_comparison_table(
	report: &mut String,
	title: &str,
	main_maps: &[BenchmarkMap],
	pr_maps: &[BenchmarkMap],
	baselines: &[NamedBenchmarkMaps],
) {
	report.push_str(title);
	report.push_str("\n\n");
	report.push_str(&format!(
		"_Paired samples: {}. RPS columns are per-side medians; vs Main is the median of per-pair changes._\n\n",
		main_maps.len()
	));
	if !baselines.is_empty() {
		report.push_str(
			"_External baseline RPS uses unpaired medians and is informational, not a Main-vs-PR signal._\n\n",
		);
	}

	let mut headers = vec![
		"Command".to_string(),
		"PR RPS median".to_string(),
		"Main RPS median".to_string(),
	];
	for (name, maps) in baselines {
		headers.push(format!(
			"{} RPS median (n={})",
			sanitize_markdown_table_text(name),
			maps.len()
		));
	}
	headers.push("vs Main median".to_string());
	headers.push("Paired spread".to_string());
	report.push_str(&format!("| {} |\n", headers.join(" | ")));
	report.push_str(&format!("|{}|\n", vec!["---"; headers.len()].join("|")));

	let mut commands = BTreeSet::new();
	for map in main_maps {
		commands.extend(map.keys());
	}
	for map in pr_maps {
		commands.extend(map.keys());
	}
	for (_, baseline_maps) in baselines {
		for map in baseline_maps {
			commands.extend(map.keys());
		}
	}

	for cmd in commands {
		let summary = summarize_paired_command(cmd, main_maps, pr_maps);
		let pr_rps_cell = summary
			.as_ref()
			.map(|value| format!("{:.2}", value.pr_rps))
			.unwrap_or_else(|| "-".to_string());
		let main_rps_cell = summary
			.as_ref()
			.map(|value| format!("{:.2}", value.main_rps))
			.unwrap_or_else(|| "-".to_string());
		let vs_main_cell = summary
			.as_ref()
			.map(|value| format_diff(value.diff_percent))
			.unwrap_or_else(|| "-".to_string());
		let spread_cell = summary
			.as_ref()
			.map(|value| {
				let noise_icon = if value.max_percent - value.min_percent > 10.0 {
					"⚠️ "
				} else {
					""
				};
				format!(
					"{noise_icon}MAD {:.2}pp; {:+.2}..{:+.2}%",
					value.mad_percent, value.min_percent, value.max_percent
				)
			})
			.unwrap_or_else(|| "-".to_string());

		let mut row = vec![cmd.to_string(), pr_rps_cell, main_rps_cell];
		for (_, baseline_maps) in baselines {
			let values = baseline_maps
				.iter()
				.filter_map(|map| map.get(cmd).copied())
				.collect::<Vec<_>>();
			let sample_count = values.len();
			let cell = median(values)
				.map(|value| {
					if sample_count == baseline_maps.len() {
						format!("{value:.2}")
					} else {
						format!("{value:.2} (n={sample_count}/{})", baseline_maps.len())
					}
				})
				.unwrap_or_else(|| "-".to_string());
			row.push(cell);
		}
		row.push(vs_main_cell);
		row.push(spread_cell);

		report.push_str(&format!("| {} |\n", row.join(" | ")));
	}
}

#[derive(Debug, PartialEq)]
struct PairedSummary {
	pr_rps: f64,
	main_rps: f64,
	diff_percent: f64,
	mad_percent: f64,
	min_percent: f64,
	max_percent: f64,
}

fn summarize_paired_command(
	command: &str,
	main_maps: &[BenchmarkMap],
	pr_maps: &[BenchmarkMap],
) -> Option<PairedSummary> {
	let samples = main_maps
		.iter()
		.zip(pr_maps)
		.map(|(main_map, pr_map)| {
			let main_rps = main_map.get(command).copied()?;
			let pr_rps = pr_map.get(command).copied()?;
			Some((main_rps, pr_rps))
		})
		.collect::<Option<Vec<_>>>()?;
	if samples.is_empty() {
		return None;
	}

	let main_rps = median(samples.iter().map(|(main, _)| *main).collect())?;
	let pr_rps = median(samples.iter().map(|(_, pr)| *pr).collect())?;
	let diffs = samples
		.iter()
		.map(|(main, pr)| ((pr - main) / main) * 100.0)
		.collect::<Vec<_>>();
	let diff_percent = median(diffs.clone())?;
	let deviations = diffs
		.iter()
		.map(|value| (value - diff_percent).abs())
		.collect();
	let mad_percent = median(deviations)?;
	let min_percent = diffs.iter().copied().fold(f64::INFINITY, f64::min);
	let max_percent = diffs.iter().copied().fold(f64::NEG_INFINITY, f64::max);

	Some(PairedSummary {
		pr_rps,
		main_rps,
		diff_percent,
		mad_percent,
		min_percent,
		max_percent,
	})
}

fn median(mut values: Vec<f64>) -> Option<f64> {
	if values.is_empty() {
		return None;
	}
	values.sort_by(f64::total_cmp);
	let middle = values.len() / 2;
	if values.len().is_multiple_of(2) {
		Some((values[middle - 1] + values[middle]) / 2.0)
	} else {
		Some(values[middle])
	}
}

fn format_diff(diff_percent: f64) -> String {
	let icon = if diff_percent > 5.0 {
		"✅ "
	} else if diff_percent < -5.0 {
		"⚠️ "
	} else {
		""
	};
	format!("{}{:+.2}%", icon, diff_percent)
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
	let re = Regex::new(r"^([A-Za-z][A-Za-z0-9_]*).*?:\s+([\d.]+)\s+requests per second(?:,|$)")
		.unwrap();

	for line in content.split(['\r', '\n']) {
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
		let baseline_second = dir.join("redis_second.txt");
		let main_pipeline = dir.join("main_pipeline.txt");
		let pr_pipeline = dir.join("pr_pipeline.txt");
		let baseline_pipeline = dir.join("redis_pipeline.txt");
		let baseline_pipeline_second = dir.join("redis_pipeline_second.txt");

		std::fs::write(&main, "SET: 100.00 requests per second\n").unwrap();
		std::fs::write(&pr, "SET: 110.00 requests per second\n").unwrap();
		std::fs::write(&baseline, "SET: 90.00 requests per second\n").unwrap();
		std::fs::write(&baseline_second, "SET: 100.00 requests per second\n").unwrap();
		std::fs::write(&main_pipeline, "GET: 200.00 requests per second\n").unwrap();
		std::fs::write(&pr_pipeline, "GET: 190.00 requests per second\n").unwrap();
		std::fs::write(&baseline_pipeline, "GET: 180.00 requests per second\n").unwrap();
		std::fs::write(
			&baseline_pipeline_second,
			"GET: 200.00 requests per second\n",
		)
		.unwrap();

		let args = Args {
			main: vec![main.display().to_string()],
			pr: vec![pr.display().to_string()],
			baselines: vec![
				format!("Redis={}", baseline.display()),
				format!("Redis={}", baseline_second.display()),
			],
			main_pipeline: vec![main_pipeline.display().to_string()],
			pr_pipeline: vec![pr_pipeline.display().to_string()],
			baseline_pipelines: vec![
				format!("Redis={}", baseline_pipeline.display()),
				format!("Redis={}", baseline_pipeline_second.display()),
			],
		};

		let report = build_report(&args).unwrap();

		assert!(report.contains("### Benchmark Comparison 🚀"));
		assert!(report.contains("### Pipeline Benchmark Comparison (-P 50) ⚡"));
		assert!(report.contains("Redis RPS median (n=2)"));
		assert!(report.contains(
			"| SET | 110.00 | 100.00 | 95.00 | ✅ +10.00% | MAD 0.00pp; +10.00..+10.00% |"
		));
		assert!(
			report.contains(
				"| GET | 190.00 | 200.00 | 190.00 | -5.00% | MAD 0.00pp; -5.00..-5.00% |"
			)
		);
		assert!(report.contains("External baseline RPS uses unpaired medians"));

		std::fs::remove_dir_all(dir).unwrap();
	}

	#[test]
	fn parse_benchmark_uses_command_token_for_custom_commands() {
		let content = "HGET bench:hash field1: 123.45 requests per second\n";
		let parsed = parse_benchmark(content);

		assert_eq!(parsed.get("HGET"), Some(&123.45));
		assert!(!parsed.contains_key("field1"));
	}

	#[test]
	fn parse_benchmark_ignores_carriage_return_progress_lines() {
		let content = concat!(
			"\rSET: rps=55540.0 (overall: 55099.2) avg_msec=1.046",
			"\rSET: 52342.32 requests per second, p50=1.063 msec\n",
			"\rSADD: rps=1000.0 (overall: 1000.0) avg_msec=1.000",
			"\rSADD: 900.25 requests per second, p50=1.100 msec\n",
		);
		let parsed = parse_benchmark(content);

		assert_eq!(parsed.get("SET"), Some(&52342.32));
		assert_eq!(parsed.get("SADD"), Some(&900.25));
		assert!(!parsed.contains_key("SET:"));
		assert!(!parsed.contains_key("SADD:"));
	}

	#[test]
	fn median_averages_the_two_middle_samples() {
		assert_eq!(median(vec![30.0, 10.0, 40.0, 20.0]), Some(25.0));
	}

	#[test]
	fn report_summarizes_paired_samples_with_median_and_mad() {
		let dir =
			std::env::temp_dir().join(format!("nimbis-xtask-paired-bench-{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();

		let mut main_files = Vec::new();
		let mut pr_files = Vec::new();
		for (index, pr_rps) in [80.0, 110.0, 90.0].into_iter().enumerate() {
			let main = dir.join(format!("main-{index}.txt"));
			let pr = dir.join(format!("pr-{index}.txt"));
			std::fs::write(&main, "SET: 100.00 requests per second\n").unwrap();
			std::fs::write(&pr, format!("SET: {pr_rps:.2} requests per second\n")).unwrap();
			main_files.push(main.display().to_string());
			pr_files.push(pr.display().to_string());
		}

		let args = Args {
			main: main_files.clone(),
			pr: pr_files.clone(),
			baselines: Vec::new(),
			main_pipeline: main_files,
			pr_pipeline: pr_files,
			baseline_pipelines: Vec::new(),
		};
		let report = build_report(&args).unwrap();

		assert!(report.contains("_Paired samples: 3."));
		assert!(
			report.contains(
				"| SET | 90.00 | 100.00 | ⚠️ -10.00% | ⚠️ MAD 10.00pp; -20.00..+10.00% |"
			)
		);

		std::fs::remove_dir_all(dir).unwrap();
	}

	#[test]
	fn report_rejects_mismatched_paired_sample_counts() {
		let args = Args {
			main: vec!["main-1.txt".into(), "main-2.txt".into()],
			pr: vec!["pr-1.txt".into()],
			baselines: Vec::new(),
			main_pipeline: vec!["main-pipeline.txt".into()],
			pr_pipeline: vec!["pr-pipeline.txt".into()],
			baseline_pipelines: Vec::new(),
		};

		let error = build_report(&args).unwrap_err();

		assert!(error.contains("Mismatched main and PR benchmark sample counts: 2 != 1"));
	}

	#[test]
	fn report_rejects_incomplete_paired_samples() {
		let dir = std::env::temp_dir().join(format!(
			"nimbis-xtask-incomplete-bench-{}",
			std::process::id()
		));
		std::fs::create_dir_all(&dir).unwrap();
		let main = dir.join("main.txt");
		let pr = dir.join("pr.txt");
		std::fs::write(
			&main,
			"SET: 100.00 requests per second\nGET: 200.00 requests per second\n",
		)
		.unwrap();
		std::fs::write(&pr, "SET: 110.00 requests per second\n").unwrap();

		let args = Args {
			main: vec![main.display().to_string()],
			pr: vec![pr.display().to_string()],
			baselines: Vec::new(),
			main_pipeline: vec![main.display().to_string()],
			pr_pipeline: vec![main.display().to_string()],
			baseline_pipelines: Vec::new(),
		};

		let error = build_report(&args).unwrap_err();

		assert!(error.contains(
			"Paired benchmark sample 1 command mismatch (only Main: [GET]; only PR: [])"
		));
		std::fs::remove_dir_all(dir).unwrap();
	}
}
