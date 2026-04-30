use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::RoutingPolicy;

/// GET command implementation
pub struct GetCmd {
	meta: CmdMeta,
}

impl Default for GetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "GET".to_string(),
				arity: 2,
				routing: RoutingPolicy::SingleKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for GetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let key = args[0].clone();

		match storage.get(key).await {
			Ok(Some(value)) => RespValue::bulk_string(value),
			Ok(None) => RespValue::Null,
			Err(e) => RespValue::error(format!("ERR {}", e)),
		}
	}
}
