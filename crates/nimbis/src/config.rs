//! Configuration module for Nimbis server
//!
//! This module provides dynamic configuration management with support for
//! both immutable and mutable configuration fields. Configuration changes
//! can trigger callbacks for side effects like reloading the log level.
//!
//! # Example
//!
//! ```no_run
//! use nimbis::config::{Cli, Parser, SERVER_CONF, setup};
//!
//! // In a real app, this would be called in main.rs
//! let args = Cli::parse();
//! setup(args);
//!
//! // Access configuration
//! let config = SERVER_CONF.load();
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;

use arc_swap::ArcSwap;
pub use clap::Parser;
pub use macros::OnlineConfig;
use serde::Deserialize;
use serde::Serialize;

/// Command-line arguments for the server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
	/// Configuration file path (TOML, JSON, or YAML).
	/// Defaults to conf/config.toml if it exists.
	#[arg(short, long)]
	pub config: Option<String>,

	/// Port to listen on
	#[arg(short, long, default_value_t = 6379)]
	pub port: u16,

	/// Host to bind to
	#[arg(long, default_value = "127.0.0.1")]
	pub host: String,

	/// Log level (trace, debug, info, warn, error)
	#[arg(short, long, default_value = "info")]
	pub log_level: String,

	/// Number of worker threads (default: number of CPU cores)
	#[arg(long)]
	pub worker_threads: Option<usize>,
}

#[derive(Debug, Clone, OnlineConfig, Deserialize, Serialize)]
#[serde(default)]
pub struct ServerConfig {
	#[online_config(immutable)]
	pub host: String,
	#[online_config(immutable)]
	pub port: u16,
	#[online_config(immutable)]
	pub data_path: String,
	// Support redis-benchmark
	#[online_config(immutable)]
	pub save: String,
	#[online_config(immutable)]
	pub appendonly: String,
	#[online_config(callback = "on_log_level_change")]
	pub log_level: String,
	#[online_config(immutable)]
	pub worker_threads: usize,
}

impl ServerConfig {
	fn on_log_level_change(&self) -> Result<(), String> {
		telemetry::logger::reload_log_level(&self.log_level).map_err(|e| e.to_string())
	}
}

impl Default for ServerConfig {
	fn default() -> Self {
		Self {
			host: "127.0.0.1".to_string(),
			port: 6379,
			data_path: "./nimbis_data".to_string(),
			save: "".to_string(),
			appendonly: "no".to_string(),
			log_level: "info".to_string(),
			worker_threads: num_cpus::get(),
		}
	}
}

pub struct GlobalConfig {
	inner: OnceLock<ArcSwap<ServerConfig>>,
}

impl Default for GlobalConfig {
	fn default() -> Self {
		Self::new()
	}
}

impl GlobalConfig {
	pub const fn new() -> Self {
		Self {
			inner: OnceLock::new(),
		}
	}

	pub fn init(&self, config: ServerConfig) {
		let _ = self.inner.set(ArcSwap::from_pointee(config));
	}

	pub fn load(&self) -> arc_swap::Guard<Arc<ServerConfig>> {
		self.inner.get().expect("Config is not initialized").load()
	}

	/// Update the configuration with a new one
	pub fn update(&self, new_config: ServerConfig) {
		self.inner
			.get()
			.expect("Config is not initialized")
			.store(Arc::new(new_config));
	}
}

pub static SERVER_CONF: GlobalConfig = GlobalConfig::new();

/// Helper macro to access server configuration fields
///
/// Usage:
/// - For Copy types (numbers): `let n = server_config!(worker_threads);`
/// - For Borrowed types (Strings): `let s = &server_config!(addr);`
#[macro_export]
macro_rules! server_config {
	($field:ident) => {
		$crate::config::SERVER_CONF.load().$field
	};
}

/// Setup configuration from CLI arguments
pub fn setup(args: Cli) {
	let mut config = if let Some(config_path) = &args.config {
		load_from_file(config_path).expect("Failed to load configuration from file")
	} else {
		let default_config = "conf/config.toml";
		if Path::new(default_config).exists() {
			load_from_file(default_config)
				.expect("Failed to load default configuration from conf/config.toml")
		} else {
			ServerConfig::default()
		}
	};

	// Override with CLI arguments if provided
	if args.host != "127.0.0.1" {
		config.host = args.host.clone();
	}

	if args.port != 6379 {
		config.port = args.port;
	}

	if args.log_level != "info" {
		config.log_level = args.log_level.clone();
	}

	if let Some(worker_threads) = args.worker_threads {
		config.worker_threads = worker_threads;
	}

	SERVER_CONF.init(config);
}

fn load_from_file<P: AsRef<Path>>(path: P) -> Result<ServerConfig, String> {
	let path = path.as_ref();
	let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;

	let extension = path
		.extension()
		.and_then(|ext| ext.to_str())
		.ok_or_else(|| "File has no extension".to_string())?;

	match extension.to_lowercase().as_str() {
		"toml" => toml::from_str(&content).map_err(|e| e.to_string()),
		"json" => serde_json::from_str(&content).map_err(|e| e.to_string()),
		"yaml" | "yml" => serde_yaml::from_str(&content).map_err(|e| e.to_string()),
		_ => Err(format!("Unsupported configuration format: {}", extension)),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_config_singleton() {
		// Initialize with default values
		let config = ServerConfig::default();

		// Try to init. If it's already initialized (by other tests), this is a no-op
		// due to our idempotent implementation.
		SERVER_CONF.init(config);

		// Now verify access via load()
		let host = &server_config!(host);
		assert_eq!(host, "127.0.0.1");

		let port = server_config!(port);
		assert_eq!(port, 6379);

		let threads = server_config!(worker_threads);
		assert_eq!(threads, num_cpus::get());
	}

	#[test]
	fn test_parse_toml() {
		let dir = tempfile::tempdir().unwrap();
		let file_path = dir.path().join("config.toml");
		let content = r#"
host = "127.0.0.1"
port = 1234
data_path = "./data"
save = "900 1"
appendonly = "yes"
log_level = "debug"
worker_threads = 4
"#;
		std::fs::write(&file_path, content).unwrap();

		let config = load_from_file(&file_path).unwrap();
		assert_eq!(config.host, "127.0.0.1");
		assert_eq!(config.port, 1234);
		assert_eq!(config.log_level, "debug");
		assert_eq!(config.worker_threads, 4);
	}

	#[test]
	fn test_parse_json() {
		let dir = tempfile::tempdir().unwrap();
		let file_path = dir.path().join("config.json");
		let content = r#"
{
  "host": "127.0.0.1",
  "port": 1234,
  "data_path": "./data",
  "save": "900 1",
  "appendonly": "yes",
  "log_level": "debug",
  "worker_threads": 4
}
"#;
		std::fs::write(&file_path, content).unwrap();

		let config = load_from_file(&file_path).unwrap();
		assert_eq!(config.host, "127.0.0.1");
		assert_eq!(config.port, 1234);
		assert_eq!(config.log_level, "debug");
	}

	#[test]
	fn test_parse_yaml() {
		let dir = tempfile::tempdir().unwrap();
		let file_path = dir.path().join("config.yaml");
		let content = r#"
host: "127.0.0.1"
port: 1234
data_path: "./data"
save: "900 1"
appendonly: "yes"
log_level: "debug"
worker_threads: 4
"#;
		std::fs::write(&file_path, content).unwrap();

		let config = load_from_file(&file_path).unwrap();
		assert_eq!(config.host, "127.0.0.1");
		assert_eq!(config.port, 1234);
		assert_eq!(config.log_level, "debug");
	}
}
