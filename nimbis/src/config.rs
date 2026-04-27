//! Configuration module for Nimbis server
//!
//! This module provides dynamic configuration management with support for
//! both immutable and mutable configuration fields. Configuration changes
//! can trigger callbacks for side effects like reloading the log level.
//!
//! # Example
//!
//! ```no_run
//! use nimbis::cli::Cli;
//! use clap::Parser;
//! use nimbis::config::{SERVER_CONF, setup};
//!
//! // In a real app, this would be called in main.rs
//! let args = Cli::parse();
//! let _telemetry_manager = setup(args)?;
//!
//! // Access configuration
//! let config = SERVER_CONF.load();
//! println!("Server address: {}:{}", config.host, config.port);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;

use arc_swap::ArcSwap;
pub use nimbis_macros::OnlineConfig;
use nimbis_telemetry::TelemetryError;
use nimbis_telemetry::logger::File as LogFile;
use nimbis_telemetry::logger::LogOutput;
use nimbis_telemetry::logger::LogRotation;
use nimbis_telemetry::logger::Terminal;
use nimbis_telemetry::manager::TELEMETRY_MANAGER;
use nimbis_telemetry::manager::TelemetryManager;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

use crate::cli::Cli;

/// Configuration-related errors
#[derive(Error, Debug)]
pub enum ConfigError {
	#[error("Failed to read configuration file '{path}': {source}")]
	Io {
		source: std::io::Error,
		path: String,
	},

	#[error("Failed to parse TOML configuration: {0}")]
	TomlParse(#[from] toml::de::Error),

	#[error("Failed to parse JSON configuration: {0}")]
	JsonParse(#[from] serde_json::Error),

	#[error("Failed to parse YAML configuration: {0}")]
	YamlParse(#[from] serde_yaml::Error),

	#[error("Unsupported configuration format: {0}")]
	UnsupportedFormat(String),

	#[error("Configuration file has no extension")]
	NoExtension,

	#[error("Failed to initialize data directory '{path}': {source}")]
	DataPathInit {
		source: std::io::Error,
		path: String,
	},

	#[error("trace_endpoint must be set when trace_enabled is true")]
	TraceEndpointRequired,

	#[error("Invalid trace_endpoint: {0}. Expected an http or https URL with a host")]
	InvalidTraceEndpoint(String),

	#[error(transparent)]
	Telemetry(#[from] TelemetryError),
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
	pub log_output: String,
	#[online_config(immutable)]
	pub log_rotation: String,
	#[online_config(immutable)]
	pub trace_enabled: bool,
	#[online_config(immutable)]
	pub trace_endpoint: String,
	#[online_config(immutable)]
	pub worker_threads: usize,
}

impl ServerConfig {
	fn on_log_level_change(&self) -> Result<(), String> {
		TELEMETRY_MANAGER
			.load()
			.reload_log_level(&self.log_level)
			.map_err(|e| e.to_string())
	}

	fn validate(&self) -> Result<(), ConfigError> {
		nimbis_telemetry::logger::validate_log_level(&self.log_level)?;

		if self.trace_enabled {
			validate_trace_endpoint(&self.trace_endpoint)?;
		}

		Ok(())
	}
}

impl Default for ServerConfig {
	fn default() -> Self {
		Self {
			host: "127.0.0.1".into(),
			port: 6379,
			data_path: "./nimbis_store".into(),
			save: "".into(),
			appendonly: "no".into(),
			log_level: "info".into(),
			log_output: "terminal".into(),
			log_rotation: "daily".into(),
			trace_enabled: false,
			trace_endpoint: "".into(),
			worker_threads: num_cpus::get(),
		}
	}
}

pub struct GlobalConfig {
	inner: OnceLock<ArcSwap<ServerConfig>>,
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

impl Default for GlobalConfig {
	fn default() -> Self {
		Self::new()
	}
}

pub static SERVER_CONF: GlobalConfig = GlobalConfig::new();

/// Helper macro to access server configuration fields
///
/// Usage:
/// - For Copy types (numbers): `let n = server_config!(worker_threads);`
/// - For Borrowed types (Strings): `let s = &server_config!(host);`
#[macro_export]
macro_rules! server_config {
	($field:ident) => {
		$crate::config::SERVER_CONF.load().$field
	};
}

pub fn setup(args: Cli) -> Result<(), ConfigError> {
	let mut config = match resolve_config_path(args.config.as_deref(), Path::new(".")) {
		Some(path) => load_from_file(path)?,
		None => ServerConfig::default(),
	};

	// Override with CLI arguments if explicitly provided
	if let Some(host) = args.host {
		config.host = host;
	}
	if let Some(port) = args.port {
		config.port = port;
	}
	if let Some(log_level) = args.log_level {
		config.log_level = log_level;
	}
	if let Some(t) = args.worker_threads {
		config.worker_threads = t;
	}

	config.validate()?;

	let log_output = resolve_log_output(&config)?;

	if log_output.is_file() {
		std::fs::create_dir_all(&config.data_path).map_err(|source| ConfigError::DataPathInit {
			path: config.data_path.clone(),
			source,
		})?;
	}

	let telemetry_manager = Arc::new(TelemetryManager::init(
		&config.log_level,
		log_output,
		config.trace_enabled,
		config.trace_endpoint.clone(),
	)?);
	TELEMETRY_MANAGER.init(telemetry_manager);
	SERVER_CONF.init(config);
	Ok(())
}

fn resolve_default_config_path_from_base(base: &Path) -> Option<PathBuf> {
	let preferred = base.join("config").join("config.toml");
	if preferred.exists() {
		return Some(preferred);
	}

	let legacy = base.join("conf").join("config.toml");
	if legacy.exists() {
		return Some(legacy);
	}

	None
}

fn resolve_config_path(explicit: Option<&Path>, base: &Path) -> Option<PathBuf> {
	if let Some(path) = explicit {
		return Some(path.to_path_buf());
	}

	resolve_default_config_path_from_base(base)
}

fn resolve_log_file_path(config: &ServerConfig) -> PathBuf {
	Path::new(&config.data_path).join("nimbis.log")
}

fn resolve_log_output(config: &ServerConfig) -> Result<LogOutput, ConfigError> {
	let log_file_path = resolve_log_file_path(config);

	match config.log_output.trim().to_ascii_lowercase().as_str() {
		"terminal" => Ok(LogOutput::Terminal(Terminal)),
		"file" => {
			let rotation = LogRotation::from_mode(&config.log_rotation)?;
			Ok(LogOutput::File(LogFile::new(log_file_path, rotation)))
		}
		_ => Err(ConfigError::from(TelemetryError::InvalidLogOutput(
			config.log_output.clone(),
		))),
	}
}

fn validate_trace_endpoint(endpoint: &str) -> Result<(), ConfigError> {
	if endpoint.is_empty() {
		return Err(ConfigError::TraceEndpointRequired);
	}
	if endpoint.trim() != endpoint {
		return Err(ConfigError::InvalidTraceEndpoint(endpoint.to_string()));
	}

	let url = url::Url::parse(endpoint)
		.map_err(|_| ConfigError::InvalidTraceEndpoint(endpoint.to_string()))?;
	if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
		return Err(ConfigError::InvalidTraceEndpoint(endpoint.to_string()));
	}

	Ok(())
}

fn load_from_file<P: AsRef<Path>>(path: P) -> Result<ServerConfig, ConfigError> {
	let path_ref = path.as_ref();
	let content = std::fs::read_to_string(path_ref).map_err(|source| ConfigError::Io {
		path: path_ref.display().to_string(),
		source,
	})?;

	let extension = path_ref
		.extension()
		.and_then(|ext| ext.to_str())
		.ok_or(ConfigError::NoExtension)?;

	let config: ServerConfig = match extension.to_lowercase().as_str() {
		"toml" => toml::from_str(&content)?,
		"json" => serde_json::from_str(&content)?,
		"yaml" | "yml" => serde_yaml::from_str(&content)?,
		_ => return Err(ConfigError::UnsupportedFormat(extension.to_string())),
	};

	config.validate()?;

	Ok(config)
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

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
log_output = "file"
log_rotation = "hourly"
trace_enabled = true
trace_endpoint = "http://localhost:4317"
worker_threads = 4
"#;
		std::fs::write(&file_path, content).unwrap();

		let config = load_from_file(&file_path).unwrap();
		assert_eq!(config.host, "127.0.0.1");
		assert_eq!(config.port, 1234);
		assert_eq!(config.log_level, "debug");
		assert_eq!(config.log_output, "file");
		assert_eq!(config.log_rotation, "hourly");
		assert!(config.trace_enabled);
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
  "log_output": "file",
  "log_rotation": "hourly",
  "trace_enabled": true,
  "trace_endpoint": "http://localhost:4317",
  "worker_threads": 4
}
"#;
		std::fs::write(&file_path, content).unwrap();

		let config = load_from_file(&file_path).unwrap();
		assert_eq!(config.host, "127.0.0.1");
		assert_eq!(config.port, 1234);
		assert_eq!(config.log_level, "debug");
		assert_eq!(config.log_output, "file");
		assert_eq!(config.log_rotation, "hourly");
		assert!(config.trace_enabled);
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
log_output: "file"
log_rotation: "hourly"
trace_enabled: true
trace_endpoint: "http://localhost:4317"
worker_threads: 4
"#;
		std::fs::write(&file_path, content).unwrap();

		let config = load_from_file(&file_path).unwrap();
		assert_eq!(config.host, "127.0.0.1");
		assert_eq!(config.port, 1234);
		assert_eq!(config.log_level, "debug");
		assert_eq!(config.log_output, "file");
		assert_eq!(config.log_rotation, "hourly");
		assert!(config.trace_enabled);
	}

	#[test]
	fn test_default_log_output() {
		assert_eq!(ServerConfig::default().log_output, "terminal");
	}

	#[test]
	fn test_default_log_rotation() {
		assert_eq!(ServerConfig::default().log_rotation, "daily");
	}

	#[test]
	fn test_default_trace_enabled() {
		assert!(!ServerConfig::default().trace_enabled);
	}

	#[test]
	fn test_trace_endpoint_required_when_trace_enabled() {
		let dir = tempfile::tempdir().unwrap();
		let file_path = dir.path().join("config.toml");
		let content = r#"
trace_enabled = true
trace_endpoint = ""
"#;
		std::fs::write(&file_path, content).unwrap();

		let err = load_from_file(&file_path).unwrap_err();
		assert!(matches!(err, ConfigError::TraceEndpointRequired));
	}

	#[rstest]
	#[case("localhost:4317")]
	#[case("grpc://localhost:4317")]
	#[case("http://")]
	#[case("http://localhost:invalid")]
	#[case(" http://localhost:4317")]
	#[case("http://localhost:4317 ")]
	fn test_trace_endpoint_must_be_valid_url(#[case] endpoint: &str) {
		let config = ServerConfig {
			trace_enabled: true,
			trace_endpoint: endpoint.into(),
			..ServerConfig::default()
		};

		let err = config.validate().unwrap_err();
		assert!(matches!(err, ConfigError::InvalidTraceEndpoint(_)));
	}

	#[rstest]
	#[case("http://localhost:4317")]
	#[case("https://collector.example.com:4317")]
	fn test_trace_endpoint_accepts_http_and_https_urls(#[case] endpoint: &str) {
		let config = ServerConfig {
			trace_enabled: true,
			trace_endpoint: endpoint.into(),
			..ServerConfig::default()
		};

		assert!(config.validate().is_ok());
	}

	#[test]
	fn test_trace_endpoint_rejects_surrounding_whitespace() {
		let dir = tempfile::tempdir().unwrap();
		let file_path = dir.path().join("config.toml");
		let content = r#"
trace_enabled = true
trace_endpoint = " http://localhost:4317 "
"#;
		std::fs::write(&file_path, content).unwrap();

		let err = load_from_file(&file_path).unwrap_err();
		assert!(matches!(err, ConfigError::InvalidTraceEndpoint(_)));
	}

	#[test]
	fn test_log_level_accepts_env_filter_expression() {
		let dir = tempfile::tempdir().unwrap();
		let file_path = dir.path().join("config.toml");
		let content = r#"
log_level = "nimbis=debug,storage=debug,resp=info,slatedb=warn,tokio=warn,info"
"#;
		std::fs::write(&file_path, content).unwrap();

		let config = load_from_file(&file_path).unwrap();
		assert_eq!(
			config.log_level,
			"nimbis=debug,storage=debug,resp=info,slatedb=warn,tokio=warn,info"
		);
	}

	#[test]
	fn test_log_level_rejects_invalid_env_filter_expression() {
		let dir = tempfile::tempdir().unwrap();
		let file_path = dir.path().join("config.toml");
		let content = r#"
log_level = "nimbis=verbose"
"#;
		std::fs::write(&file_path, content).unwrap();

		let err = load_from_file(&file_path).unwrap_err();
		assert!(
			matches!(err, ConfigError::Telemetry(TelemetryError::InvalidLogLevel(v)) if v == "nimbis=verbose")
		);
	}

	#[test]
	fn test_resolve_log_file_path() {
		let config = ServerConfig::default();

		assert_eq!(
			resolve_log_file_path(&config),
			Path::new(&config.data_path).join("nimbis.log")
		);
	}

	#[test]
	fn test_resolve_terminal_log_output() {
		let output = resolve_log_output(&ServerConfig::default()).unwrap();
		assert!(matches!(output, LogOutput::Terminal(_)));
	}

	#[test]
	fn test_resolve_file_log_output() {
		let config = ServerConfig {
			log_output: "file".into(),
			log_rotation: "hourly".into(),
			data_path: "./custom_data".into(),
			..ServerConfig::default()
		};

		let output = resolve_log_output(&config).unwrap();
		assert!(matches!(output, LogOutput::File(_)));
		assert!(output.is_file());
	}

	#[test]
	fn test_resolve_terminal_log_output_ignores_invalid_rotation() {
		let config = ServerConfig {
			log_output: "terminal".into(),
			log_rotation: "invalid".into(),
			..ServerConfig::default()
		};

		let output = resolve_log_output(&config).unwrap();
		assert!(matches!(output, LogOutput::Terminal(_)));
	}

	#[test]
	fn test_default_config_prefers_config_dir() {
		let dir = tempfile::tempdir().unwrap();
		let preferred_dir = dir.path().join("config");
		let legacy_dir = dir.path().join("conf");
		std::fs::create_dir_all(&preferred_dir).unwrap();
		std::fs::create_dir_all(&legacy_dir).unwrap();
		std::fs::write(preferred_dir.join("config.toml"), "host = \"127.0.0.2\"").unwrap();
		std::fs::write(legacy_dir.join("config.toml"), "host = \"127.0.0.3\"").unwrap();

		let path = resolve_default_config_path_from_base(dir.path()).unwrap();
		assert_eq!(path, preferred_dir.join("config.toml"));
	}

	#[test]
	fn test_default_config_falls_back_to_legacy_conf_dir() {
		let dir = tempfile::tempdir().unwrap();
		let legacy_dir = dir.path().join("conf");
		std::fs::create_dir_all(&legacy_dir).unwrap();
		std::fs::write(legacy_dir.join("config.toml"), "host = \"127.0.0.3\"").unwrap();

		let path = resolve_default_config_path_from_base(dir.path()).unwrap();
		assert_eq!(path, legacy_dir.join("config.toml"));
	}

	#[test]
	fn test_default_config_returns_none_when_missing() {
		let dir = tempfile::tempdir().unwrap();

		let path = resolve_default_config_path_from_base(dir.path());
		assert!(path.is_none());
	}

	#[test]
	fn test_explicit_config_path_overrides_default_paths() {
		let dir = tempfile::tempdir().unwrap();
		let preferred_dir = dir.path().join("config");
		std::fs::create_dir_all(&preferred_dir).unwrap();
		std::fs::write(preferred_dir.join("config.toml"), "host = \"127.0.0.2\"").unwrap();
		let explicit = dir.path().join("custom.toml");
		std::fs::write(&explicit, "host = \"127.0.0.9\"").unwrap();

		let path = resolve_config_path(Some(explicit.as_path()), dir.path()).unwrap();
		assert_eq!(path, explicit);
	}
}
