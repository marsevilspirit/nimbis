use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

#[derive(Debug, Clone)]
pub struct TtlCmd {
	meta: CmdMeta,
}

impl Default for TtlCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "ttl".to_string(),
				arity: 2, // TTL key
			},
		}
	}
}

#[async_trait]
impl Cmd for TtlCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		match storage.ttl(key).await {
			Ok(Some(ttl_ms)) => RespValue::Integer(match ttl_ms {
				-1 => -1,
				_ => ttl_ms / 1000,
			}),
			Ok(None) => RespValue::Integer(-2), // Key does not exist
			Err(e) => RespValue::Error(Bytes::from(e.to_string())),
		}
	}
}
