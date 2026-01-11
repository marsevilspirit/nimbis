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

impl Default for ZRangeCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "ZRANGE".to_string(),
				arity: -4, // ZRANGE key start stop [WITHSCORES]
			},
		}
	}
}

#[async_trait]
impl Cmd for ZRangeCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();

		// Helper closure to parse integer arguments
		let parse_int = |arg: &Bytes| -> Result<isize, RespValue> {
			String::from_utf8_lossy(arg)
				.parse::<isize>()
				.map_err(|_| RespValue::error("ERR value is not an integer or out of range"))
		};

		let start = match parse_int(&args[1]) {
			Ok(v) => v,
			Err(e) => return e,
		};

		let stop = match parse_int(&args[2]) {
			Ok(v) => v,
			Err(e) => return e,
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
