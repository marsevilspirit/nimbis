use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use crate::cmd::Cmd;
use crate::cmd::CmdMeta;

pub struct RPushCmd {
	meta: CmdMeta,
}

impl Default for RPushCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "RPUSH".to_string(),
				arity: -3, // RPUSH key element [element ...]
			},
		}
	}
}

#[async_trait]
impl Cmd for RPushCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		let elements = args[1..].to_vec();

		match storage.rpush(key, elements).await {
			Ok(len) => RespValue::Integer(len as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
