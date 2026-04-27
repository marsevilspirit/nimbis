use std::path::PathBuf;

use clap::Parser;

/// Command-line arguments for the server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
	/// Configuration file path (TOML, JSON, or YAML).
	/// Defaults to config/config.toml (or legacy conf/config.toml) if present.
	#[arg(short, long, value_hint = clap::ValueHint::FilePath)]
	pub config: Option<PathBuf>,

	/// Port to listen on
	#[arg(short, long)]
	pub port: Option<u16>,

	/// Host to bind to
	#[arg(long, value_hint = clap::ValueHint::Hostname)]
	pub host: Option<String>,

	/// Log level (trace, debug, info, warn, error)
	#[arg(short, long, value_parser = ["trace", "debug", "info", "warn", "error"])]
	pub log_level: Option<String>,

	/// Number of worker threads (default: number of CPU cores)
	#[arg(long)]
	pub worker_threads: Option<usize>,
}
