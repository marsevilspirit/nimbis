use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

/// PING command implementation
pub(super) async fn execute(_storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	match args.len() {
		0 => RespValue::simple_string("PONG"),
		1 => RespValue::bulk_string(args[0].clone()),
		_ => RespValue::error("ERR wrong number of arguments for 'ping' command"),
	}
}
