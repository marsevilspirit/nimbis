use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct SaddCmd {
	meta: CmdMeta,
}

impl Default for SaddCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SADD".to_string(),
				arity: -3,
			},
		}
	}
}

#[async_trait]
impl Cmd for SaddCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		let members = args[1..].to_vec();

		match storage.sadd(key, members).await {
			Ok(count) => RespValue::Integer(count as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
