use std::sync::Arc;

use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

/// GET command implementation
pub struct GetCmd {
	meta: CmdMeta,
}

impl Default for GetCmd {
	fn default() -> Self {
		Self::new()
	}
}

impl GetCmd {
	pub fn new() -> Self {
		Self {
			meta: CmdMeta {
				name: "GET".to_string(),
				arity: 2,
			},
		}
	}
}

#[async_trait]
impl Cmd for GetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue {
		let key = args[0].clone();

		match storage.get(key).await {
			Ok(Some(value)) => RespValue::bulk_string(value),
			Ok(None) => RespValue::Null,
			Err(e) => RespValue::error(format!("ERR {}", e)),
		}
	}
}
