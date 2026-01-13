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
	if args.len() < 3 {
		eprintln!(
			"Usage: {} <main_benchmark_file> <pr_benchmark_file> [redis_benchmark_file]",
			args[0]
		);
		process::exit(1);
	}

	let main_file = &args[1];
	let pr_file = &args[2];
	let redis_file = if args.len() > 3 { Some(&args[3]) } else { None };

	let main_content = fs::read_to_string(main_file).expect("Failed to read main file");
	let pr_content = fs::read_to_string(pr_file).expect("Failed to read pr file");
	let redis_content = redis_file.map(|f| fs::read_to_string(f).expect("Failed to read redis file"));

	let main_map = parse_benchmark(&main_content);
	let pr_map = parse_benchmark(&pr_content);
	let redis_map = redis_content.as_ref().map(|c| parse_benchmark(c));

	println!("### Benchmark Comparison 🚀");
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
	if let Some(r_map) = &redis_map {
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
		
		let pr_icon = if pr_diff_percent > 5.0 { "✅ " } else if pr_diff_percent < -5.0 { "⚠️ " } else { "" };
		let vs_main_cell = if main_rps > 0.0 {
			format!("{}{:+.2}%", pr_icon, pr_diff_percent)
		} else {
			"-".to_string()
		};

		if let Some(r_map) = &redis_map {
			let redis_rps = r_map.get(cmd).copied().unwrap_or(0.0);
			
			// Calculate vs Redis diff
			let redis_diff_percent = if redis_rps > 0.0 {
				((pr_rps - redis_rps) / redis_rps) * 100.0
			} else if pr_rps > 0.0 {
				100.0
			} else {
				0.0
			};
			
			let redis_icon = if redis_diff_percent > 0.0 { "🏆 " } else { "" };
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
			let cmd = caps.get(1).unwrap().as_str();
			let rps_str = caps.get(2).unwrap().as_str();
			if let Ok(rps) = rps_str.parse::<f64>() {
				map.insert(cmd.to_string(), rps);
			}
		}
	}
	map
}
