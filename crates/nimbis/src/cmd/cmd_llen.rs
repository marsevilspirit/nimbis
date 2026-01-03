use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use crate::cmd::Cmd;
use crate::cmd::CmdMeta;

pub struct LLenCmd {
	meta: CmdMeta,
}

impl LLenCmd {
	pub fn new() -> Self {
		Self {
			meta: CmdMeta {
				name: "LLEN".to_string(),
				arity: 2, // LLEN key
			},
		}
	}
}

impl Default for LLenCmd {
	fn default() -> Self {
		Self::new()
	}
}

#[async_trait]
impl Cmd for LLenCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		match storage.llen(key).await {
			Ok(len) => RespValue::Integer(len as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
