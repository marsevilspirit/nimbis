use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = args[0].clone();
	let elements = args[1..].to_vec();

	match storage.rpush(key, elements).await {
		Ok(len) => RespValue::Integer(len as i64),
		Err(e) => RespValue::error(e.to_string()),
	}
}
