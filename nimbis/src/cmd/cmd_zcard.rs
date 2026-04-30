use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::RoutingPolicy;

pub struct ZCardCmd {
	meta: CmdMeta,
}

impl Default for ZCardCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "ZCARD".to_string(),
				arity: 2, // ZCARD key
				routing: RoutingPolicy::SingleKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for ZCardCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let key = args[0].clone();

		match storage.zcard(key).await {
			Ok(count) => RespValue::integer(count as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
