use std::sync::Arc;

use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct HMGetCmd {
	meta: CmdMeta,
}

impl Default for HMGetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "HMGET".to_string(),
				arity: -3, // HMGET key field [field ...]
			},
		}
	}
}

#[async_trait]
impl Cmd for HMGetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue {
		let key = &args[0];
		let fields = &args[1..];

		match storage.hmget(key.clone(), fields).await {
			Ok(values) => {
				let array: Vec<RespValue> = values
					.into_iter()
					.map(|v| match v {
						Some(bytes) => RespValue::bulk_string(bytes),
						None => RespValue::Null,
					})
					.collect();
				RespValue::array(array)
			}
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
