use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = &args[0];
	let fields = &args[1..];

	match storage.hmget(key.clone(), fields).await {
		Ok(values) => {
			let array: Vec<RespValue> = values
				.into_iter()
				.map(|v| match v {
					Some(bytes) => RespValue::bulk_string(bytes),
					None => RespValue::Null,
				})
				.collect();
			RespValue::array(array)
		}
		Err(e) => RespValue::error(e.to_string()),
	}
}
