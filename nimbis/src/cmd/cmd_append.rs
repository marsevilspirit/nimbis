use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = args[0].clone();
	let append_val = args[1].clone();

	match storage.append(key, append_val).await {
		Ok(len) => RespValue::Integer(len as i64),
		Err(err) => RespValue::Error(Bytes::from(err.to_string())),
	}
}
