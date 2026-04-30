use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;
use crate::cmd::Cmd;
use crate::cmd::CmdMeta;
use crate::cmd::RoutingPolicy;

pub struct LLenCmd {
	meta: CmdMeta,
}

impl Default for LLenCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "LLEN".to_string(),
				arity: 2, // LLEN key
				routing: RoutingPolicy::SingleKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for LLenCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let key = args[0].clone();
		match storage.llen(key).await {
			Ok(len) => RespValue::Integer(len as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
