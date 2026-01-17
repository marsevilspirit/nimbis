use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use crate::cmd::Cmd;
use crate::cmd::CmdMeta;

pub struct LPushCmd {
	meta: CmdMeta,
}

impl Default for LPushCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "LPUSH".to_string(),
				arity: -3, // LPUSH key element [element ...]
			},
		}
	}
}

#[async_trait]
impl Cmd for LPushCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		let elements = args[1..].to_vec();

		match storage.lpush(key, elements).await {
			Ok(len) => RespValue::Integer(len as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
