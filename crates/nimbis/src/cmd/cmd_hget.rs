use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct HGetCmd {
	meta: CmdMeta,
}

impl Default for HGetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "HGET".to_string(),
				arity: 3, // HGET key field
			},
		}
	}
}

#[async_trait]
impl Cmd for HGetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[bytes::Bytes]) -> RespValue {
		let key = &args[0];
		let field = &args[1];

		match storage.hget(key.clone(), field.clone()).await {
			Ok(Some(val)) => RespValue::bulk_string(val),
			Ok(None) => RespValue::Null,
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
