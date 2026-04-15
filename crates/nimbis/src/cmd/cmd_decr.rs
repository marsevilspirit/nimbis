use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;

pub struct DecrCmd {
	meta: CmdMeta,
}

impl Default for DecrCmd {
	fn default() -> Self {
		DecrCmd {
			meta: CmdMeta {
				name: "DECR".to_string(),
				arity: 2,
			},
		}
	}
}

#[async_trait]
impl Cmd for DecrCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let key = args[0].clone();

		match storage.decr(key).await {
			Ok(val) => RespValue::Integer(val),
			Err(err) => RespValue::Error(Bytes::from(err.to_string())),
		}
	}
}
