use std::sync::Arc;

use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

#[derive(Debug)]
pub struct HDelCmd {
	meta: CmdMeta,
}

impl Default for HDelCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "HDEL".to_string(),
				arity: -3,
			},
		}
	}
}

#[async_trait]
impl Cmd for HDelCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue {
		let key = args[0].clone();
		let fields = &args[1..];

		match storage.hdel(key, fields).await {
			Ok(count) => RespValue::Integer(count),
			Err(e) => RespValue::Error(e.to_string().into()),
		}
	}
}
