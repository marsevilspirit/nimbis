use std::collections::HashMap;
use std::sync::Arc;

use super::AppendCmd;
use super::ClientCmd;
use super::Cmd;
use super::ConfigCmd;
use super::DecrCmd;
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
use super::HelloCmd;
use super::IncrCmd;
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

const COMMAND_NAME_STACK_CAPACITY: usize = 32;

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
		inner.insert("HELLO", Arc::new(HelloCmd::default()));
		// string type cmd
		inner.insert("SET", Arc::new(SetCmd::default()));
		inner.insert("GET", Arc::new(GetCmd::default()));
		inner.insert("DEL", Arc::new(DelCmd::default()));
		inner.insert("EXISTS", Arc::new(ExistsCmd::default()));
		inner.insert("INCR", Arc::new(IncrCmd::default()));
		inner.insert("DECR", Arc::new(DecrCmd::default()));
		inner.insert("APPEND", Arc::new(AppendCmd::default()));
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
		inner.insert("CONFIG", Arc::new(ConfigCmd::default()));
		inner.insert("CLIENT", Arc::new(ClientCmd::default()));
		// other type cmd
		inner.insert("FLUSHDB", Arc::new(FlushDbCmd::default()));
		Self { inner }
	}

	pub fn get_cmd(&self, name: &str) -> Option<&Arc<dyn Cmd>> {
		if let Some(cmd) = self.inner.get(name) {
			return Some(cmd);
		}

		if name.len() <= COMMAND_NAME_STACK_CAPACITY {
			let mut uppercase = [0; COMMAND_NAME_STACK_CAPACITY];
			uppercase[..name.len()].copy_from_slice(name.as_bytes());
			uppercase[..name.len()].make_ascii_uppercase();
			let uppercase = std::str::from_utf8(&uppercase[..name.len()])
				.expect("ASCII uppercasing preserves UTF-8");
			return self.inner.get(uppercase);
		}

		self.inner.iter().find_map(|(registered_name, cmd)| {
			registered_name.eq_ignore_ascii_case(name).then_some(cmd)
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn command_lookup_is_case_insensitive() {
		let table = CmdTable::new();

		for name in ["PING", "ping", "pInG"] {
			let cmd = table.get_cmd(name).unwrap();
			assert_eq!(cmd.meta().name, "PING");
		}
	}

	#[test]
	fn long_unknown_command_is_not_found() {
		let table = CmdTable::new();
		let name = "x".repeat(COMMAND_NAME_STACK_CAPACITY + 1);

		assert!(table.get_cmd(&name).is_none());
	}
}
