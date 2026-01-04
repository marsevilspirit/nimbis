use std::collections::HashMap;
use std::sync::Arc;

use super::Cmd;
use super::ConfigGroupCmd;
use super::DelCmd;
use super::ExistsCmd;
use super::ExpireCmd;
use super::GetCmd;
use super::HGetAllCmd;
use super::HGetCmd;
use super::HLenCmd;
use super::HMGetCmd;
use super::HSetCmd;
use super::LLenCmd;
use super::LPopCmd;
use super::LPushCmd;
use super::LRangeCmd;
use super::PingCmd;
use super::RPopCmd;
use super::RPushCmd;
use super::SaddCmd;
use super::ScardCmd;
use super::SetCmd;
use super::SismemberCmd;
use super::SmembersCmd;
use super::SremCmd;
use super::TtlCmd;

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
		inner.insert("HSET".to_string(), Arc::new(HSetCmd::new()));
		inner.insert("HGET".to_string(), Arc::new(HGetCmd::new()));
		inner.insert("HLEN".to_string(), Arc::new(HLenCmd::new()));
		inner.insert("HMGET".to_string(), Arc::new(HMGetCmd::new()));
		inner.insert("HGETALL".to_string(), Arc::new(HGetAllCmd::new()));
		// list type cmd
		inner.insert("LPUSH".to_string(), Arc::new(LPushCmd::new()));
		inner.insert("RPUSH".to_string(), Arc::new(RPushCmd::new()));
		inner.insert("LPOP".to_string(), Arc::new(LPopCmd::new()));
		inner.insert("RPOP".to_string(), Arc::new(RPopCmd::new()));
		inner.insert("LLEN".to_string(), Arc::new(LLenCmd::new()));
		inner.insert("LRANGE".to_string(), Arc::new(LRangeCmd::new()));
		// set type cmd
		inner.insert("SADD".to_string(), Arc::new(SaddCmd::new()));
		inner.insert("SMEMBERS".to_string(), Arc::new(SmembersCmd::new()));
		inner.insert("SISMEMBER".to_string(), Arc::new(SismemberCmd::new()));
		inner.insert("SREM".to_string(), Arc::new(SremCmd::new()));
		inner.insert("SCARD".to_string(), Arc::new(ScardCmd::new()));
		// expire type cmd
		inner.insert("EXPIRE".to_string(), Arc::new(ExpireCmd::default()));
		inner.insert("TTL".to_string(), Arc::new(TtlCmd::default()));
		// config type cmd
		inner.insert("CONFIG".to_string(), Arc::new(ConfigGroupCmd::new()));
		Self { inner }
	}

	pub fn get_cmd(&self, name: &str) -> Option<&Arc<dyn Cmd>> {
		self.inner.get(name)
	}
}
