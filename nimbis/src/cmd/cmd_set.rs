use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

/// SET command implementation
pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = args[0].clone();
	let value = args[1].clone();

	match storage.set(key, value).await {
		Ok(_) => RespValue::simple_string("OK"),
		Err(e) => RespValue::error(format!("ERR {}", e)),
	}
}
