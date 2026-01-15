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

#[async_trait::async_trait]
impl Cmd for FlushDbCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &storage::Storage, _args: &[bytes::Bytes]) -> resp::RespValue {
		// FLUSHDB effectively means deleting everything.
		// Since we don't have a direct "drop db" in slatedb easily exposed or if we want to keep the valid instances.
		// Storage struct has a flush_all method.
		match storage.flush_all().await {
			Ok(_) => resp::RespValue::SimpleString("OK".into()),
			Err(e) => resp::RespValue::error(e.to_string()),
		}
	}
}
