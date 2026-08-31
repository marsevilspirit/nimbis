use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = args[0].clone();
	let fields = &args[1..];

	match storage.hdel(key, fields).await {
		Ok(count) => RespValue::Integer(count),
		Err(e) => RespValue::Error(e.to_string().into()),
	}
}
