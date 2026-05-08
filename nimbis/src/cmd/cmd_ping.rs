use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::CommandKind;
use super::KeySpec;

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
				key_spec: KeySpec::None,
				kind: CommandKind::Local,
			},
		}
	}
}

#[async_trait]
impl Cmd for PingCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		match args.len() {
			0 => RespValue::simple_string("PONG"),
			1 => RespValue::bulk_string(args[0].clone()),
			_ => RespValue::error("ERR wrong number of arguments for 'ping' command"),
		}
	}
}
