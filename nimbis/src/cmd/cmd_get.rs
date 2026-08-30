use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

/// GET command implementation
pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = args[0].clone();

	match storage.get(key).await {
		Ok(Some(value)) => RespValue::bulk_string(value),
		Ok(None) => RespValue::Null,
		Err(e) => RespValue::error(format!("ERR {}", e)),
	}
}
