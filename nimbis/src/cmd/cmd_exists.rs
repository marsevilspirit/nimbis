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
use crate::coordinator::CoordinatedCommandPlan;
use crate::coordinator::ScatterRequest;

pub struct ExistsCmd {
	meta: CmdMeta,
}

impl Default for ExistsCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "EXISTS".to_string(),
				arity: -2,
				routing: RoutingPolicy::MultiKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for ExistsCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	fn plan(&self, args: &[Bytes], _worker_count: usize) -> Result<CommandPlan, RespValue> {
		Ok(CoordinatedCommandPlan::Scatter {
			subrequests: args
				.iter()
				.map(|key| ScatterRequest {
					route_key: key.clone(),
					request: ParsedCmd {
						name: self.meta.name.clone(),
						args: vec![key.clone()],
					},
					output_index: None,
				})
				.collect(),
			aggregate: AggregatePolicy::IntegerSum,
		}
		.into())
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		if let Some(key) = args.first() {
			match storage.exists(key.clone()).await {
				Ok(exists) => RespValue::Integer(if exists { 1 } else { 0 }),
				Err(e) => RespValue::Error(Bytes::from(e.to_string())),
			}
		} else {
			RespValue::Integer(0)
		}
	}
}
