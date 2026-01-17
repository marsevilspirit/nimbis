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
				arity: 0,
			},
		}
	}
}

#[async_trait]
impl Cmd for FlushDbCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, _args: &[Bytes]) -> RespValue {
		// FLUSHDB removes all keys from the current database.
		// Storage provides a flush_all method to delete all data while keeping the storage instances valid.
		match storage.flush_all().await {
			Ok(_) => RespValue::SimpleString("OK".into()),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
