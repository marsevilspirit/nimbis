use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::RoutingPolicy;

pub struct HGetCmd {
	meta: CmdMeta,
}

impl Default for HGetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "HGET".to_string(),
				arity: 3, // HGET key field
				routing: RoutingPolicy::SingleKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for HGetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let key = &args[0];
		let field = &args[1];

		match storage.hget(key.clone(), field.clone()).await {
			Ok(Some(val)) => RespValue::bulk_string(val),
			Ok(None) => RespValue::Null,
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
