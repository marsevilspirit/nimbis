use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::CommandKind;
use super::KeySpec;
use super::cmd_sunion::set_member_subrequests;
use crate::coordinator::AggregatePolicy;
use crate::coordinator::CommandPlan;
use crate::coordinator::CoordinatedCommandPlan;

pub struct SinterCmd {
	meta: CmdMeta,
}

impl Default for SinterCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SINTER".to_string(),
				arity: -2,
				key_spec: KeySpec::All,
				kind: CommandKind::Read,
			},
		}
	}
}

#[async_trait]
impl Cmd for SinterCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	fn plan(&self, args: &[Bytes], _worker_count: usize) -> Result<CommandPlan, RespValue> {
		Ok(CoordinatedCommandPlan::Scatter {
			subrequests: set_member_subrequests(args),
			aggregate: AggregatePolicy::SetIntersection,
		}
		.into())
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
