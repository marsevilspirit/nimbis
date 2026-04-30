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

pub struct SdiffCmd {
	meta: CmdMeta,
}

impl Default for SdiffCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SDIFF".to_string(),
				arity: -2,
				routing: RoutingPolicy::MultiKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for SdiffCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	fn plan(&self, args: &[Bytes]) -> Result<CommandPlan, RespValue> {
		Ok(CommandPlan::Scatter {
			subrequests: set_member_subrequests(args),
			aggregate: AggregatePolicy::SetDifference,
		})
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let Some(first_key) = args.first() else {
			return RespValue::Array(Vec::new());
		};

		let mut result = match storage.smembers(first_key.clone()).await {
			Ok(values) => values.into_iter().collect::<std::collections::HashSet<_>>(),
			Err(e) => return RespValue::error(e.to_string()),
		};

		for key in &args[1..] {
			match storage.smembers(key.clone()).await {
				Ok(values) => {
					let set = values.into_iter().collect::<std::collections::HashSet<_>>();
					result = result.difference(&set).cloned().collect();
				}
				Err(e) => return RespValue::error(e.to_string()),
			}
		}

		let mut members: Vec<_> = result.into_iter().collect();
		members.sort();
		RespValue::Array(members.into_iter().map(RespValue::BulkString).collect())
	}
}
