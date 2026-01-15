use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct HGetAllCmd {
	meta: CmdMeta,
}

impl Default for HGetAllCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "HGETALL".to_string(),
				arity: 2, // HGETALL key
			},
		}
	}
}

#[async_trait]
impl Cmd for HGetAllCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[bytes::Bytes]) -> RespValue {
		let key = &args[0];

		match storage.hgetall(key.clone()).await {
			Ok(pairs) => {
				let mut array = Vec::with_capacity(pairs.len() * 2);
				for (field, value) in pairs {
					array.push(RespValue::bulk_string(field));
					array.push(RespValue::bulk_string(value));
				}
				RespValue::array(array)
			}
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
