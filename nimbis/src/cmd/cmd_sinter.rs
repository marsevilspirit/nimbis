use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::RoutingPolicy;
use super::cmd_sunion::set_member_subrequests;
use crate::coordinator::AggregatePolicy;
use crate::coordinator::CommandPlan;

pub struct SinterCmd {
	meta: CmdMeta,
}

impl Default for SinterCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SINTER".to_string(),
				arity: -2,
				routing: RoutingPolicy::MultiKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for SinterCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	fn plan(&self, args: &[Bytes]) -> Result<CommandPlan, RespValue> {
		Ok(CommandPlan::Scatter {
			subrequests: set_member_subrequests(args),
			aggregate: AggregatePolicy::SetIntersection,
		})
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let mut sets = Vec::new();
		for key in args {
			match storage.smembers(key.clone()).await {
				Ok(values) => {
					sets.push(values.into_iter().collect::<std::collections::HashSet<_>>())
				}
				Err(e) => return RespValue::error(e.to_string()),
			}
		}

		let Some(first) = sets.first().cloned() else {
			return RespValue::Array(Vec::new());
		};
		let result = sets
			.into_iter()
			.skip(1)
			.fold(first, |acc, set| acc.intersection(&set).cloned().collect());
		let mut members: Vec<_> = result.into_iter().collect();
		members.sort();
		RespValue::Array(members.into_iter().map(RespValue::BulkString).collect())
	}
}
