use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct SmembersCmd {
	meta: CmdMeta,
}

impl Default for SmembersCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SMEMBERS".to_string(),
				arity: 2,
			},
		}
	}
}

#[async_trait]
impl Cmd for SmembersCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();

		match storage.smembers(key).await {
			Ok(members) => {
				let resp_members: Vec<RespValue> =
					members.into_iter().map(RespValue::bulk_string).collect();
				RespValue::Array(resp_members)
			}
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
