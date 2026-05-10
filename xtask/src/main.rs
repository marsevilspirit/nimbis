use std::path::Path;

use clap::Parser;
use clap::Subcommand;
use xtask::benchmarks;
use xtask::checks;
use xtask::redis_benchmark;
use xtask::write_stderr_line;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
	/// Check Cargo.toml workspace dependency conventions.
	CheckWorkspace,
	/// Check repository-specific Rust formatting conventions.
	CheckCodeFmt,
	/// Check Rust code for numbered step comments.
	CheckNumberedComments,
	/// Compare benchmark outputs and print a Markdown report.
	CompareBenchmarks(benchmarks::Args),
	/// Run redis-benchmark against a running Nimbis server.
	RedisBenchmark(redis_benchmark::Args),
}

fn main() {
	let cli = Cli::parse();
	let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.expect("xtask manifest should be inside the workspace root");

	let result = execute(cli.command, workspace_root);

	if let Err(error) = result {
		write_stderr_line(&error);
		std::process::exit(1);
	}
}

fn execute(command: Command, workspace_root: &Path) -> Result<(), String> {
	match command {
		Command::CheckWorkspace => checks::check_workspace(workspace_root),
		Command::CheckCodeFmt => checks::check_code_fmt(workspace_root),
		Command::CheckNumberedComments => checks::check_numbered_comments(workspace_root),
		Command::CompareBenchmarks(args) => benchmarks::compare_benchmarks(args),
		Command::RedisBenchmark(args) => redis_benchmark::run(args, workspace_root),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn execute_routes_redis_benchmark_command() {
		let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
			.parent()
			.expect("xtask manifest should be inside the workspace root");
		let result = execute(
			Command::RedisBenchmark(redis_benchmark::Args {
				redis_benchmark: Some("/definitely-missing-redis-benchmark".into()),
				redis_cli: Some("/definitely-missing-redis-cli".into()),
				..redis_benchmark::Args::default()
			}),
			workspace_root,
		);

		assert!(result.is_err());
		assert!(
			result
				.unwrap_err()
				.contains("required command '/definitely-missing-redis-benchmark' was not found")
		);
	}
}
