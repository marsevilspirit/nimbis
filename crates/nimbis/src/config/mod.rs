use std::sync::Arc;
use std::sync::OnceLock;

use arc_swap::ArcSwap;
use smart_default::SmartDefault;

#[derive(Debug, SmartDefault)]
pub struct ServerConfig {
	#[default = "127.0.0.1:6379"]
	pub addr: String,
	#[default = "./nimbis_data"]
	pub data_path: String,
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

		// Try to init. If it's already initialized (by other tests), this is a no-op due to our idempotent implementation.
		SERVER_CONF.init(config);

		// Now verify access via load()
		assert_eq!(SERVER_CONF.load().addr, "127.0.0.1:6379");
	}
}
