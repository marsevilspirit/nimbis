use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct HLenCmd {
	meta: CmdMeta,
}

impl Default for HLenCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "HLEN".to_string(),
				arity: 2, // HLEN key
			},
		}
	}
}

#[async_trait]
impl Cmd for HLenCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[bytes::Bytes]) -> RespValue {
		let key = &args[0];

		match storage.hlen(key.clone()).await {
			Ok(len) => RespValue::integer(len as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
