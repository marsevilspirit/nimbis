use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct ScardCmd {
	meta: CmdMeta,
}

impl Default for ScardCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SCARD".to_string(),
				arity: 2,
			},
		}
	}
}

#[async_trait]
impl Cmd for ScardCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();

		match storage.scard(key).await {
			Ok(count) => RespValue::Integer(count as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
