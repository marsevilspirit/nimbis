use std::collections::HashMap;
use std::sync::Arc;

use super::Cmd;
use super::ConfigGroupCmd;
use super::DelCmd;
use super::ExistsCmd;
use super::ExpireCmd;
use super::FlushDbCmd;
use super::GetCmd;
use super::HDelCmd;
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
use super::ZAddCmd;
use super::ZCardCmd;
use super::ZRangeCmd;
use super::ZRemCmd;
use super::ZScoreCmd;

pub struct CmdTable {
	inner: HashMap<&'static str, Arc<dyn Cmd>>,
}

impl Default for CmdTable {
	fn default() -> Self {
		Self::new()
	}
}

impl CmdTable {
	pub fn new() -> Self {
		let mut inner: HashMap<&'static str, Arc<dyn Cmd>> = HashMap::new();
		// ping cmd
		inner.insert("PING", Arc::new(PingCmd::default()));
		// string type cmd
		inner.insert("SET", Arc::new(SetCmd::default()));
		inner.insert("GET", Arc::new(GetCmd::default()));
		inner.insert("DEL", Arc::new(DelCmd::default()));
		inner.insert("EXISTS", Arc::new(ExistsCmd::default()));
		// hash type cmd
		inner.insert("HSET", Arc::new(HSetCmd::default()));
		inner.insert("HDEL", Arc::new(HDelCmd::default()));
		inner.insert("HGET", Arc::new(HGetCmd::default()));
		inner.insert("HLEN", Arc::new(HLenCmd::default()));
		inner.insert("HMGET", Arc::new(HMGetCmd::default()));
		inner.insert("HGETALL", Arc::new(HGetAllCmd::default()));
		// list type cmd
		inner.insert("LPUSH", Arc::new(LPushCmd::default()));
		inner.insert("RPUSH", Arc::new(RPushCmd::default()));
		inner.insert("LPOP", Arc::new(LPopCmd::default()));
		inner.insert("ZADD", Arc::new(ZAddCmd::default()));
		inner.insert("ZRANGE", Arc::new(ZRangeCmd::default()));
		inner.insert("ZSCORE", Arc::new(ZScoreCmd::default()));
		inner.insert("ZREM", Arc::new(ZRemCmd::default()));
		inner.insert("ZCARD", Arc::new(ZCardCmd::default()));
		inner.insert("LLEN", Arc::new(LLenCmd::default()));
		inner.insert("LRANGE", Arc::new(LRangeCmd::default()));
		inner.insert("RPOP", Arc::new(RPopCmd::default()));
		// set type cmd
		inner.insert("SADD", Arc::new(SaddCmd::default()));
		inner.insert("SMEMBERS", Arc::new(SmembersCmd::default()));
		inner.insert("SISMEMBER", Arc::new(SismemberCmd::default()));
		inner.insert("SREM", Arc::new(SremCmd::default()));
		inner.insert("SCARD", Arc::new(ScardCmd::default()));
		// expire type cmd
		inner.insert("EXPIRE", Arc::new(ExpireCmd::default()));
		inner.insert("TTL", Arc::new(TtlCmd::default()));
		// config type cmd
		inner.insert("CONFIG", Arc::new(ConfigGroupCmd::default()));
		// other type cmd
		inner.insert("FLUSHDB", Arc::new(FlushDbCmd::default()));
		Self { inner }
	}

	pub fn get_cmd(&self, name: &str) -> Option<&Arc<dyn Cmd>> {
		self.inner.get(name)
	}
}
