use std::sync::Arc;
use std::sync::OnceLock;

use arc_swap::ArcSwap;

#[derive(Debug)]
pub struct NimbisConfig {
	pub addr: String,
	pub data_path: String,
}

impl Default for NimbisConfig {
	fn default() -> Self {
		Self {
			addr: "127.0.0.1:6379".to_string(),
			data_path: "./nimbis_data".to_string(),
		}
	}
}

pub struct GlobalConfig {
	inner: OnceLock<ArcSwap<NimbisConfig>>,
}

impl GlobalConfig {
	pub const fn new() -> Self {
		Self {
			inner: OnceLock::new(),
		}
	}

	pub fn init(&self, config: NimbisConfig) {
		let _ = self.inner.set(ArcSwap::from_pointee(config));
	}

	pub fn load(&self) -> arc_swap::Guard<Arc<NimbisConfig>> {
		self.inner.get().expect("Config is not initialized").load()
	}
}

pub static SERVER_CONF: GlobalConfig = GlobalConfig::new();

pub fn init_config() {
	SERVER_CONF.init(NimbisConfig::default());
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_config_singleton() {
		// Initialize with default values
		let config = NimbisConfig::default();

		// Try to init. If it's already initialized (by other tests), this is a no-op due to our idempotent implementation.
		SERVER_CONF.init(config);

		// Now verify access via load()
		assert_eq!(SERVER_CONF.load().addr, "127.0.0.1:6379");
	}
}
