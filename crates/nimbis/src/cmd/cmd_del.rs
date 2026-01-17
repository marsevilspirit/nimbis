use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct DelCmd {
	meta: CmdMeta,
}

impl Default for DelCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "DEL".to_string(),
				arity: 2, // Exactly 1 key
			},
		}
	}
}

#[async_trait]
impl Cmd for DelCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[bytes::Bytes]) -> RespValue {
		// TODO: Support multi-key deletion via scatter-gather across workers
		// (similar to FLUSHDB broadcast pattern, not MGET/MSET which are for get/set)
		if let Some(key) = args.first() {
			match storage.del(key.clone()).await {
				Ok(true) => RespValue::Integer(1),
				Ok(false) => RespValue::Integer(0),
				Err(e) => RespValue::error(e.to_string()),
			}
		} else {
			RespValue::Integer(0)
		}
	}
}
