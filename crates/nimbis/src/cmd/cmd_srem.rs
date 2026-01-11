use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct SremCmd {
	meta: CmdMeta,
}

impl Default for SremCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SREM".to_string(),
				arity: -3,
			},
		}
	}
}

#[async_trait]
impl Cmd for SremCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		let members = args[1..].to_vec();

		match storage.srem(key, members).await {
			Ok(count) => RespValue::Integer(count as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
