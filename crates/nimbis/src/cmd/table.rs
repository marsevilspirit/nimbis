use std::collections::HashMap;
use std::sync::Arc;

use super::Cmd;
use super::ConfigGroupCmd;
use super::GetCmd;
use super::PingCmd;
use super::SetCmd;

pub struct CmdTable {
	inner: HashMap<String, Arc<dyn Cmd>>,
}

impl Default for CmdTable {
	fn default() -> Self {
		Self::new()
	}
}

impl CmdTable {
	pub fn new() -> Self {
		let mut inner: HashMap<String, Arc<dyn Cmd>> = HashMap::new();
		// ping cmd
		inner.insert("PING".to_string(), Arc::new(PingCmd::new()));
		// string type cmd
		inner.insert("SET".to_string(), Arc::new(SetCmd::new()));
		inner.insert("GET".to_string(), Arc::new(GetCmd::new()));
		// hash type cmd
		inner.insert("HSET".to_string(), Arc::new(super::HSetCmd::new()));
		inner.insert("HGET".to_string(), Arc::new(super::HGetCmd::new()));
		inner.insert("HLEN".to_string(), Arc::new(super::HLenCmd::new()));
		inner.insert("HMGET".to_string(), Arc::new(super::HMGetCmd::new()));
		inner.insert("HGETALL".to_string(), Arc::new(super::HGetAllCmd::new()));
		// config type cmd
		inner.insert("CONFIG".to_string(), Arc::new(ConfigGroupCmd::new()));
		Self { inner }
	}

	pub fn get_cmd(&self, name: &str) -> Option<&Arc<dyn Cmd>> {
		self.inner.get(name)
	}
}
