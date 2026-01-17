use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct ZRemCmd {
	meta: CmdMeta,
}

impl Default for ZRemCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "ZREM".to_string(),
				arity: -3, // ZREM key member [member ...]
			},
		}
	}
}

#[async_trait]
impl Cmd for ZRemCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		let members = args[1..].to_vec();

		match storage.zrem(key, members).await {
			Ok(removed) => RespValue::integer(removed as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
