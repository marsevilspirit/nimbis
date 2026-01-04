use std::collections::HashMap;
use std::sync::Arc;

use super::Cmd;
use super::ConfigGroupCmd;
use super::DelCmd;
use super::ExistsCmd;
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
		inner.insert("DEL".to_string(), Arc::new(DelCmd::default()));
		inner.insert("EXISTS".to_string(), Arc::new(ExistsCmd::default()));
		// hash type cmd
		inner.insert("HSET".to_string(), Arc::new(super::HSetCmd::new()));
		inner.insert("HGET".to_string(), Arc::new(super::HGetCmd::new()));
		inner.insert("HLEN".to_string(), Arc::new(super::HLenCmd::new()));
		inner.insert("HMGET".to_string(), Arc::new(super::HMGetCmd::new()));
		inner.insert("HGETALL".to_string(), Arc::new(super::HGetAllCmd::new()));
		// list type cmd
		inner.insert("LPUSH".to_string(), Arc::new(super::LPushCmd::new()));
		inner.insert("RPUSH".to_string(), Arc::new(super::RPushCmd::new()));
		inner.insert("LPOP".to_string(), Arc::new(super::LPopCmd::new()));
		inner.insert("RPOP".to_string(), Arc::new(super::RPopCmd::new()));
		inner.insert("LLEN".to_string(), Arc::new(super::LLenCmd::new()));
		inner.insert("LRANGE".to_string(), Arc::new(super::LRangeCmd::new()));
		// set type cmd
		inner.insert("SADD".to_string(), Arc::new(super::SaddCmd::new()));
		inner.insert("SMEMBERS".to_string(), Arc::new(super::SmembersCmd::new()));
		inner.insert(
			"SISMEMBER".to_string(),
			Arc::new(super::SismemberCmd::new()),
		);
		inner.insert("SREM".to_string(), Arc::new(super::SremCmd::new()));
		inner.insert("SCARD".to_string(), Arc::new(super::ScardCmd::new()));
		// expire type cmd
		inner.insert("EXPIRE".to_string(), Arc::new(super::ExpireCmd::default()));
		inner.insert("TTL".to_string(), Arc::new(super::TtlCmd::default()));
		// config type cmd
		inner.insert("CONFIG".to_string(), Arc::new(ConfigGroupCmd::new()));
		Self { inner }
	}

	pub fn get_cmd(&self, name: &str) -> Option<&Arc<dyn Cmd>> {
		self.inner.get(name)
	}
}
