use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::RoutingPolicy;

pub struct IncrCmd {
	meta: CmdMeta,
}

impl Default for IncrCmd {
	fn default() -> Self {
		IncrCmd {
			meta: CmdMeta {
				name: "INCR".to_string(),
				arity: 2,
				routing: RoutingPolicy::SingleKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for IncrCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let key = args[0].clone();

		match storage.incr(key).await {
			Ok(val) => RespValue::Integer(val),
			Err(err) => RespValue::Error(Bytes::from(err.to_string())),
		}
	}
}
