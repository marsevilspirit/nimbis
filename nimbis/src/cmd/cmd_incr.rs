use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = args[0].clone();

	match storage.incr(key).await {
		Ok(val) => RespValue::Integer(val),
		Err(err) => RespValue::Error(Bytes::from(err.to_string())),
	}
}
