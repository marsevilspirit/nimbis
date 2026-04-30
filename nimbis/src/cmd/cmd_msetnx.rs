use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::RoutingPolicy;
use super::cmd_mset::pairs_from_args;
use crate::coordinator::CommandPlan;
use crate::coordinator::LockedExecution;

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
			},
		}
	}
}

#[async_trait]
impl Cmd for MSetNxCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	fn plan(&self, args: &[Bytes]) -> Result<CommandPlan, RespValue> {
		if !args.len().is_multiple_of(2) {
			return Err(RespValue::error("ERR syntax error"));
		}

		let pairs = pairs_from_args(args);
		let keys = pairs.iter().map(|(key, _)| key.clone()).collect();
		Ok(CommandPlan::LockedMultiKey {
			keys,
			execution: LockedExecution::MSetNx { pairs },
		})
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		if !args.len().is_multiple_of(2) {
			return RespValue::error("ERR syntax error");
		}

		match storage.msetnx(pairs_from_args(args)).await {
			Ok(written) => RespValue::Integer(if written { 1 } else { 0 }),
			Err(e) => RespValue::error(format!("ERR {}", e)),
		}
	}
}
