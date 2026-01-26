//! Configuration module for Nimbis server
//!
//! This module provides dynamic configuration management with support for
//! both immutable and mutable configuration fields. Configuration changes
//! can trigger callbacks for side effects like reloading the log level.
//!
//! # Example
//!
//! ```no_run
//! use config::{init_config, SERVER_CONF};
//!
//! // Initialize with default configuration
//! init_config();
//!
//! // Access configuration
//! let config = SERVER_CONF.load();
//! println!("Server address: {}", config.addr);
//! ```

use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;

use arc_swap::ArcSwap;
pub use clap::Parser;
pub use config_derive::OnlineConfig;

/// Command-line arguments for the server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
	/// Port to listen on
	#[arg(short, long, default_value_t = 6379)]
	pub port: u16,

	/// Host to bind to
	#[arg(long, default_value = "127.0.0.1")]
	pub host: String,

	/// Log level (trace, debug, info, warn, error)
	#[arg(short, long, default_value = "info")]
	pub log_level: String,
}

#[derive(Debug, Clone, OnlineConfig)]
pub struct ServerConfig {
	#[online_config(immutable)]
	pub addr: String,
	#[online_config(immutable)]
	pub data_path: String,
	// Support redis-benchmark
	#[online_config(immutable)]
	pub save: String,
	#[online_config(immutable)]
	pub appendonly: String,
	#[online_config(callback = "on_log_level_change")]
	pub log_level: String,
}

impl ServerConfig {
	fn on_log_level_change(&self) -> Result<(), String> {
		telemetry::logger::reload_log_level(&self.log_level).map_err(|e| e.to_string())
	}
}

impl Default for ServerConfig {
	fn default() -> Self {
		Self {
			addr: "127.0.0.1:6379".to_string(),
			data_path: "./nimbis_data".to_string(),
			save: "".to_string(),
			appendonly: "no".to_string(),
			log_level: "info".to_string(),
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
/// Usage: `server_config!(field_name)`
#[macro_export]
macro_rules! server_config {
	($field:ident) => {
		&$crate::SERVER_CONF.load().$field
	};
}

/// Setup configuration from CLI arguments
pub fn setup(args: Cli) {
	let addr = format!("{}:{}", args.host, args.port);

	let config = ServerConfig {
		addr,
		log_level: args.log_level.clone(),
		..ServerConfig::default()
	};

	SERVER_CONF.init(config);
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
		assert_eq!(*server_config!(addr), "127.0.0.1:6379");
	}
}
