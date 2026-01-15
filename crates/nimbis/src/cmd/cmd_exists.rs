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
				arity: -2, // At least 1 key
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
		let mut count = 0;
		for key in args {
			match storage.exists(key.clone()).await {
				Ok(exists) => {
					if exists {
						count += 1;
					}
				}
				Err(e) => return RespValue::Error(Bytes::from(e.to_string())),
			}
		}
		RespValue::Integer(count)
	}
}
