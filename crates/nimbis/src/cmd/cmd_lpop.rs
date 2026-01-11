use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use crate::cmd::Cmd;
use crate::cmd::CmdMeta;
use crate::cmd::utils;

pub struct LPopCmd {
	meta: CmdMeta,
}

impl Default for LPopCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "LPOP".to_string(),
				arity: -2, // LPOP key [count]
			},
		}
	}
}

#[async_trait]
impl Cmd for LPopCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		let count = if args.len() > 1 {
			match utils::parse_int(&args[1]) {
				Ok(n) => Some(n),
				Err(e) => return RespValue::error(e),
			}
		} else {
			None
		};

		match storage.lpop(key, count).await {
			Ok(elements) => {
				if elements.is_empty() {
					return RespValue::Null;
				}

				if args.len() == 1 {
					// Single pop
					// The `elements.is_empty()` check above ensures `elements` has exactly one item here
					// so we can safely unwrap.
					RespValue::bulk_string(elements.first().unwrap().clone())
				} else {
					// Count pop
					let resp_elements: Vec<RespValue> =
						elements.into_iter().map(RespValue::bulk_string).collect();
					RespValue::Array(resp_elements)
				}
			}
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
