use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;
use crate::cmd::Cmd;
use crate::cmd::CmdMeta;
use crate::cmd::utils;

pub struct LRangeCmd {
	meta: CmdMeta,
}

impl Default for LRangeCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "LRANGE".to_string(),
				arity: 4, // LRANGE key start stop
			},
		}
	}
}

#[async_trait]
impl Cmd for LRangeCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let key = args[0].clone();

		let start = match utils::parse_int(&args[1]) {
			Ok(n) => n,
			Err(e) => return RespValue::error(e),
		};

		let stop = match utils::parse_int(&args[2]) {
			Ok(n) => n,
			Err(e) => return RespValue::error(e),
		};

		match storage.lrange(key, start, stop).await {
			Ok(elements) => {
				let resp_elements: Vec<RespValue> =
					elements.into_iter().map(RespValue::bulk_string).collect();
				RespValue::Array(resp_elements)
			}
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
