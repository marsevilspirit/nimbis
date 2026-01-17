use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct ExistsCmd {
	meta: CmdMeta,
}

impl Default for ExistsCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "EXISTS".to_string(),
				arity: 2, // Exactly 1 key (multi-key requires scatter-gather across workers)
			},
		}
	}
}

#[async_trait]
impl Cmd for ExistsCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes]) -> RespValue {
		// TODO: Support multi-key existence check via scatter-gather across workers
		if let Some(key) = args.first() {
			match storage.exists(key.clone()).await {
				Ok(exists) => RespValue::Integer(if exists { 1 } else { 0 }),
				Err(e) => RespValue::Error(Bytes::from(e.to_string())),
			}
		} else {
			RespValue::Integer(0)
		}
	}
}
