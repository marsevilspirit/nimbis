use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct SismemberCmd {
	meta: CmdMeta,
}

impl Default for SismemberCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SISMEMBER".to_string(),
				arity: 3,
			},
		}
	}
}

#[async_trait]
impl Cmd for SismemberCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		let member = args[1].clone();

		match storage.sismember(key, member).await {
			Ok(exists) => RespValue::Integer(if exists { 1 } else { 0 }),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
