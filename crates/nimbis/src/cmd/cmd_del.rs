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
				arity: -2, // At least 1 key
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
		let mut count = 0;
		for key in args {
			match storage.del(key.clone()).await {
				Ok(deleted) => {
					if deleted {
						count += 1;
					}
				}
				Err(e) => return RespValue::error(e.to_string()),
			}
		}
		RespValue::Integer(count)
	}
}
