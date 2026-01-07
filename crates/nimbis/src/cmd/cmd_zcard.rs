use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct ZCardCmd {
	meta: CmdMeta,
}

impl ZCardCmd {
	pub fn new() -> Self {
		Self {
			meta: CmdMeta {
				name: "ZCARD".to_string(),
				arity: 2, // ZCARD key
			},
		}
	}
}

impl Default for ZCardCmd {
	fn default() -> Self {
		Self::new()
	}
}

#[async_trait]
impl Cmd for ZCardCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();

		match storage.zcard(key).await {
			Ok(count) => RespValue::integer(count as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
