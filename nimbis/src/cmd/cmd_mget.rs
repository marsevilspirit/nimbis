use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::CommandKind;
use super::KeySpec;

pub struct MGetCmd {
	meta: CmdMeta,
}

impl Default for MGetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "MGET".to_string(),
				arity: -2,
				key_spec: KeySpec::All,
				kind: CommandKind::Read,
			},
		}
	}
}

#[async_trait]
impl Cmd for MGetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let mut values = Vec::with_capacity(args.len());
		for key in args {
			match storage.get(key.clone()).await {
				Ok(Some(value)) => values.push(RespValue::BulkString(value)),
				Ok(None) => values.push(RespValue::Null),
				Err(e) => return RespValue::error(format!("ERR {}", e)),
			}
		}
		RespValue::Array(values)
	}
}
