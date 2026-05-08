use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::CommandKind;
use super::KeySpec;
use super::RoutingPolicy;
use super::cmd_mset::pairs_from_args;

pub struct MSetNxCmd {
	meta: CmdMeta,
}

impl Default for MSetNxCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "MSETNX".to_string(),
				arity: -3,
				routing: RoutingPolicy::MultiKey,
				key_spec: KeySpec::Step { first: 0, step: 2 },
				kind: CommandKind::Write,
			},
		}
	}
}

#[async_trait]
impl Cmd for MSetNxCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		if !args.len().is_multiple_of(2) {
			return RespValue::error("ERR wrong number of arguments for 'msetnx' command");
		}

		match storage.msetnx(pairs_from_args(args)).await {
			Ok(written) => RespValue::Integer(if written { 1 } else { 0 }),
			Err(e) => RespValue::error(format!("ERR {}", e)),
		}
	}
}
