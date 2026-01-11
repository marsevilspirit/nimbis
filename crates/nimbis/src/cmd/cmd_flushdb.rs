use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct FlushDbCmd {
	meta: CmdMeta,
}

impl Default for FlushDbCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "FLUSHDB".to_string(),
				arity: 1, // FLUSHDB
			},
		}
	}
}

#[async_trait]
impl Cmd for FlushDbCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, _args: &[Bytes]) -> RespValue {
		match storage.flush_all().await {
			Ok(_) => RespValue::simple_string("OK"),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
