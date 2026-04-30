use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::ParsedCmd;
use super::RoutingPolicy;
use crate::coordinator::AggregatePolicy;
use crate::coordinator::CommandPlan;
use crate::coordinator::ScatterRequest;

pub struct SunionCmd {
	meta: CmdMeta,
}

impl Default for SunionCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SUNION".to_string(),
				arity: -2,
				routing: RoutingPolicy::MultiKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for SunionCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	fn plan(&self, args: &[Bytes]) -> Result<CommandPlan, RespValue> {
		Ok(CommandPlan::Scatter {
			subrequests: set_member_subrequests(args),
			aggregate: AggregatePolicy::SetUnion,
		})
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

pub(super) fn set_member_subrequests(args: &[Bytes]) -> Vec<ScatterRequest> {
	args.iter()
		.enumerate()
		.map(|(idx, key)| ScatterRequest {
			route_key: key.clone(),
			request: ParsedCmd {
				name: "SMEMBERS".to_string(),
				args: vec![key.clone()],
			},
			output_index: Some(idx),
		})
		.collect()
}
