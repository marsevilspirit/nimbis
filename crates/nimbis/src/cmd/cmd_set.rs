use std::sync::Arc;

use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

/// SET command implementation
pub struct SetCmd {
	meta: CmdMeta,
}

impl Default for SetCmd {
	fn default() -> Self {
		Self::new()
	}
}

impl SetCmd {
	pub fn new() -> Self {
		Self {
			meta: CmdMeta {
				name: "SET".to_string(),
				arity: 3,
			},
		}
	}
}

#[async_trait]
impl Cmd for SetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue {
		let key = args[0].clone();
		let value = args[1].clone();

		match storage.set(key, value).await {
			Ok(_) => RespValue::simple_string("OK"),
			Err(e) => RespValue::error(format!("ERR {}", e)),
		}
	}
}
