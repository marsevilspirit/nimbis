use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct IncrCmd {
	meta: CmdMeta,
}

impl Default for IncrCmd {
	fn default() -> Self {
		IncrCmd {
			meta: CmdMeta {
				name: "INCR".to_string(),
				arity: 2,
			},
		}
	}
}

#[async_trait]
impl Cmd for IncrCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[bytes::Bytes]) -> RespValue {
		let key = args[0].clone();

		match storage.incr(key).await {
			Ok(val) => RespValue::Integer(val),
			Err(err) => RespValue::Error(bytes::Bytes::from(err.to_string())),
		}
	}
}
