use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
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
