use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

/// PING command implementation
pub struct PingCmd {
	meta: CmdMeta,
}

impl Default for PingCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "PING".to_string(),
				arity: -1, // Allow 0 or 1 argument
			},
		}
	}
}

#[async_trait]
impl Cmd for PingCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, args: &[bytes::Bytes]) -> RespValue {
		match args.len() {
			0 => RespValue::simple_string("PONG"),
			1 => RespValue::bulk_string(args[0].clone()),
			_ => RespValue::error("ERR wrong number of arguments for 'ping' command"),
		}
	}
}
