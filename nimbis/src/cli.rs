use std::path::PathBuf;

use clap::Parser;

/// Command-line arguments for the server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
	/// Configuration file path (TOML, JSON, or YAML).
	/// If not provided, nimbis only checks `config/config.toml`, if present.
	#[arg(short, long, value_hint = clap::ValueHint::FilePath)]
	pub config: Option<PathBuf>,

	/// Port to listen on
	#[arg(short, long)]
	pub port: Option<u16>,

	/// Host to bind to
	#[arg(long, value_hint = clap::ValueHint::Hostname)]
	pub host: Option<String>,

	/// Log level/filter expression (EnvFilter syntax, e.g. "nimbis=debug,info")
	#[arg(short, long)]
	pub log_level: Option<String>,

	/// Number of Tokio runtime worker threads (default: number of CPU cores)
	#[arg(long)]
	pub runtime_threads: Option<usize>,
}

#[cfg(test)]
mod tests {
	use clap::Parser;

	use super::Cli;

	#[test]
	fn parses_runtime_threads() {
		let cli = Cli::parse_from(["nimbis", "--runtime-threads", "4"]);

		assert_eq!(cli.runtime_threads, Some(4));
	}
}
