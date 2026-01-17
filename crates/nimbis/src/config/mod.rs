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
}

impl Default for ServerConfig {
	fn default() -> Self {
		Self {
			addr: "127.0.0.1:6379".to_string(),
			data_path: "./nimbis_data".to_string(),
			save: "".to_string(),
			appendonly: "no".to_string(),
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
