use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::CommandKind;
use super::KeySpec;

pub struct SunionCmd {
	meta: CmdMeta,
}

impl Default for SunionCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SUNION".to_string(),
				arity: -2,
				key_spec: KeySpec::All,
				kind: CommandKind::Read,
			},
		}
	}
}

#[async_trait]
impl Cmd for SunionCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let mut members = std::collections::HashSet::new();
		for key in args {
			match storage.smembers(key.clone()).await {
				Ok(values) => members.extend(values),
				Err(e) => return RespValue::error(e.to_string()),
			}
		}
		let mut members: Vec<_> = members.into_iter().collect();
		members.sort();
		RespValue::Array(members.into_iter().map(RespValue::BulkString).collect())
	}
}
