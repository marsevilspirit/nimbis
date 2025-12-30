use std::collections::HashMap;
use std::sync::Arc;

use super::Cmd;
use super::ConfigCommandGroup;
use super::GetCommand;
use super::PingCommand;
use super::SetCommand;

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
		inner.insert("SET".to_string(), Arc::new(SetCommand::new()));
		inner.insert("GET".to_string(), Arc::new(GetCommand::new()));
		inner.insert("PING".to_string(), Arc::new(PingCommand::new()));
		inner.insert("CONFIG".to_string(), Arc::new(ConfigCommandGroup::new()));
		Self { inner }
	}

	pub fn get_cmd(&self, name: &str) -> Option<&Arc<dyn Cmd>> {
		self.inner.get(name)
	}
}
