use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct AppendCmd {
	meta: CmdMeta,
}

impl Default for AppendCmd {
	fn default() -> Self {
		AppendCmd {
			meta: CmdMeta {
				name: "APPEND".to_string(),
				arity: 3,
			},
		}
	}
}

#[async_trait]
impl Cmd for AppendCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[bytes::Bytes]) -> RespValue {
		let key = args[0].clone();
		let append_val = args[1].clone();

		match storage.append(key, append_val).await {
			Ok(len) => RespValue::Integer(len as i64),
			Err(err) => RespValue::Error(bytes::Bytes::from(err.to_string())),
		}
	}
}
