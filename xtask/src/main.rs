use std::path::Path;

use clap::Parser;
use clap::Subcommand;
use xtask::benchmarks;
use xtask::checks;
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
}

fn main() {
	let cli = Cli::parse();
	let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.expect("xtask manifest should be inside the workspace root");

	let result = match cli.command {
		Command::CheckWorkspace => checks::check_workspace(workspace_root),
		Command::CheckCodeFmt => checks::check_code_fmt(workspace_root),
		Command::CheckNumberedComments => checks::check_numbered_comments(workspace_root),
		Command::CompareBenchmarks(args) => benchmarks::compare_benchmarks(args),
	};

	if let Err(error) = result {
		write_stderr_line(&error);
		std::process::exit(1);
	}
}
