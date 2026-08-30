use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;
use crate::cmd::utils;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
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
