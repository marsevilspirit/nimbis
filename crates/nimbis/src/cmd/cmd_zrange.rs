use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct ZRangeCmd {
	meta: CmdMeta,
}

impl ZRangeCmd {
	pub fn new() -> Self {
		Self {
			meta: CmdMeta {
				name: "ZRANGE".to_string(),
				arity: -4, // ZRANGE key start stop [WITHSCORES]
			},
		}
	}
}

impl Default for ZRangeCmd {
	fn default() -> Self {
		Self::new()
	}
}

#[async_trait]
impl Cmd for ZRangeCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		let start_str = String::from_utf8_lossy(&args[1]);
		let stop_str = String::from_utf8_lossy(&args[2]);

		let start = match start_str.parse::<isize>() {
			Ok(v) => v,
			Err(_) => {
				return RespValue::error("ERR value is not an integer or out of range");
			}
		};

		let stop = match stop_str.parse::<isize>() {
			Ok(v) => v,
			Err(_) => {
				return RespValue::error("ERR value is not an integer or out of range");
			}
		};

		let mut with_scores = false;
		if args.len() > 3 {
			let opt = String::from_utf8_lossy(&args[3]).to_uppercase();
			if opt == "WITHSCORES" {
				with_scores = true;
			} else {
				return RespValue::error("ERR syntax error");
			}
		}

		match storage.zrange(key, start, stop, with_scores).await {
			Ok(members) => RespValue::array(members.into_iter().map(RespValue::bulk_string)),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
