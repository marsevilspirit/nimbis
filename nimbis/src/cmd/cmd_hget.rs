use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = &args[0];
	let field = &args[1];

	match storage.hget(key.clone(), field.clone()).await {
		Ok(Some(val)) => RespValue::bulk_string(val),
		Ok(None) => RespValue::Null,
		Err(e) => RespValue::error(e.to_string()),
	}
}
