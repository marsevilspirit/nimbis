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

pub struct DelCmd {
	meta: CmdMeta,
}

impl Default for DelCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "DEL".to_string(),
				arity: -2,
				routing: RoutingPolicy::MultiKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for DelCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	fn plan(&self, args: &[Bytes]) -> Result<CommandPlan, RespValue> {
		Ok(CommandPlan::Scatter {
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
		})
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		if let Some(key) = args.first() {
			match storage.del(key.clone()).await {
				Ok(true) => RespValue::Integer(1),
				Ok(false) => RespValue::Integer(0),
				Err(e) => RespValue::error(e.to_string()),
			}
		} else {
			RespValue::Integer(0)
		}
	}
}
