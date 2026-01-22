//! Configuration module for Nimbis server
//!
//! This module provides dynamic configuration management with support for
//! both immutable and mutable configuration fields. Configuration changes
//! can trigger callbacks for side effects like reloading the log level.
//!
//! # Example
//!
//! ```no_run
//! use nimbis::config::{init_config, SERVER_CONF};
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
use config::OnlineConfig;

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

impl ServerConfig {
	/// Callback invoked when log_level configuration changes.
	///
	/// This method is called by the OnlineConfig derive macro when the
	/// log_level field is updated. It triggers a reload of the logging
	/// subsystem with the new level.
	///
	/// # Errors
	///
	/// Returns an error if the log level is invalid or if the reload fails.
	fn on_log_level_change(&self) -> Result<(), String> {
		telemetry::reload_log_level(&self.log_level).map_err(|e| e.to_string())
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

pub fn init_config() {
	SERVER_CONF.init(ServerConfig::default());
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
		assert_eq!(SERVER_CONF.load().addr, "127.0.0.1:6379");
	}
}
