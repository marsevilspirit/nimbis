use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::CommandKind;
use super::KeySpec;

pub struct ExistsCmd {
	meta: CmdMeta,
}

impl Default for ExistsCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "EXISTS".to_string(),
				arity: -2,
				key_spec: KeySpec::All,
				kind: CommandKind::Read,
			},
		}
	}
}

#[async_trait]
impl Cmd for ExistsCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let mut count = 0;
		for key in args {
			match storage.exists(key.clone()).await {
				Ok(true) => count += 1,
				Ok(false) => {}
				Err(e) => return RespValue::Error(Bytes::from(e.to_string())),
			}
		}
		RespValue::Integer(count)
	}
}
