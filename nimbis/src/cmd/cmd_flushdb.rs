use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, _args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	// FLUSHDB removes all keys from the current database.
	// Storage provides a flush_all method to delete all data while keeping the
	// storage instances valid.
	match storage.flush_all().await {
		Ok(_) => RespValue::simple_string("OK"),
		Err(e) => RespValue::error(e.to_string()),
	}
}
