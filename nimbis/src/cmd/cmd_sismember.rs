use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::RoutingPolicy;

pub struct SismemberCmd {
	meta: CmdMeta,
}

impl Default for SismemberCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SISMEMBER".to_string(),
				arity: 3,
				routing: RoutingPolicy::SingleKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for SismemberCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let key = args[0].clone();
		let member = args[1].clone();

		match storage.sismember(key, member).await {
			Ok(exists) => RespValue::Integer(if exists { 1 } else { 0 }),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
