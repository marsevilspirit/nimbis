use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = args[0].clone();
	let member = args[1].clone();

	match storage.sismember(key, member).await {
		Ok(exists) => RespValue::Integer(if exists { 1 } else { 0 }),
		Err(e) => RespValue::error(e.to_string()),
	}
}
