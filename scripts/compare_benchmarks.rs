#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! regex = "1"
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;

use regex::Regex;

fn main() {
	let args: Vec<String> = env::args().collect();
	if args.len() != 3 {
		eprintln!(
			"Usage: {} <main_benchmark_file> <pr_benchmark_file>",
			args[0]
		);
		process::exit(1);
	}

	let main_file = &args[1];
	let pr_file = &args[2];

	let main_content = fs::read_to_string(main_file).expect("Failed to read main file");
	let pr_content = fs::read_to_string(pr_file).expect("Failed to read pr file");

	let main_map = parse_benchmark(&main_content);
	let pr_map = parse_benchmark(&pr_content);

	println!("### Benchmark Comparison 🚀");
	println!();
	println!("| Command | Main RPS | PR RPS | Diff |");
	println!("|---|---|---|---|");

	// Collect all commands
	let mut commands: Vec<&String> = main_map.keys().chain(pr_map.keys()).collect();
	commands.sort();
	commands.dedup();

	for cmd in commands {
		let main_rps = main_map.get(cmd).copied().unwrap_or(0.0);
		let pr_rps = pr_map.get(cmd).copied().unwrap_or(0.0);

		let diff_percent = if main_rps > 0.0 {
			((pr_rps - main_rps) / main_rps) * 100.0
		} else if pr_rps > 0.0 {
			100.0 // Treated as new/inf increase
		} else {
			0.0
		};

		// Icon for diff
		let icon = if diff_percent > 5.0 {
			"✅" // Improved
		} else if diff_percent < -5.0 {
			"⚠️" // Regressed
		} else {
			"➖" // No major change
		};

		println!(
			"| {} | {:.2} | {:.2} | {} {:.2}% |",
			cmd, main_rps, pr_rps, icon, diff_percent
		);
	}

	println!();
	println!("*Comparison triggered by automated benchmark.*");
}

fn parse_benchmark(content: &str) -> HashMap<String, f64> {
	let mut map = HashMap::new();
	// Regex to match "SET: 12345.67 requests per second"
	// Adjust regex if format varies, but usually `redis-benchmark -q` outputs straightforward lines.
	// Sometimes it might strip the colon or be key="value".
	// Standard `redis-benchmark -q` output:
	// PING_INLINE: 106382.98 requests per second
	// SET: 104166.66 requests per second
	let re = Regex::new(r"([\w_]+):\s+([\d\.]+)\s+requests per second").unwrap();

	for line in content.lines() {
		if let Some(caps) = re.captures(line) {
			let cmd = caps.get(1).map_or("", |m| m.as_str());
			let rps_str = caps.get(2).map_or("0", |m| m.as_str());
			if let Ok(rps) = rps_str.parse::<f64>() {
				map.insert(cmd.to_string(), rps);
			}
		}
	}
	map
}
