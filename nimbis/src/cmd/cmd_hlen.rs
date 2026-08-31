use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = &args[0];

	match storage.hlen(key.clone()).await {
		Ok(len) => RespValue::integer(len as i64),
		Err(e) => RespValue::error(e.to_string()),
	}
}
