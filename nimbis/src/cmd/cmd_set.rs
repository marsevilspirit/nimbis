use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::RoutingPolicy;

/// SET command implementation
pub struct SetCmd {
	meta: CmdMeta,
}

impl Default for SetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SET".to_string(),
				arity: 3,
				routing: RoutingPolicy::SingleKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for SetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let key = args[0].clone();
		let value = args[1].clone();

		match storage.set(key, value).await {
			Ok(_) => RespValue::simple_string("OK"),
			Err(e) => RespValue::error(format!("ERR {}", e)),
		}
	}
}
