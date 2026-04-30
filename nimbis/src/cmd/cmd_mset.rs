use std::collections::HashMap;

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
use crate::coordinator::hash_key;

pub struct MSetCmd {
	meta: CmdMeta,
}

impl Default for MSetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "MSET".to_string(),
				arity: -3,
				routing: RoutingPolicy::MultiKey,
			},
		}
	}
}

#[async_trait]
impl Cmd for MSetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	fn plan(&self, args: &[Bytes]) -> Result<CommandPlan, RespValue> {
		if !args.len().is_multiple_of(2) {
			return Err(RespValue::error("ERR syntax error"));
		}

		let mut grouped: HashMap<u64, Vec<Bytes>> = HashMap::new();
		for pair in args.chunks_exact(2) {
			let key = pair[0].clone();
			let value = pair[1].clone();
			grouped
				.entry(hash_key(&key))
				.or_default()
				.extend([key, value]);
		}

		Ok(CommandPlan::Scatter {
			subrequests: grouped
				.into_values()
				.map(|args| ScatterRequest {
					route_key: args[0].clone(),
					request: ParsedCmd {
						name: self.meta.name.clone(),
						args,
					},
					output_index: None,
				})
				.collect(),
			aggregate: AggregatePolicy::AllOk,
		})
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		if !args.len().is_multiple_of(2) {
			return RespValue::error("ERR syntax error");
		}

		match storage.mset(pairs_from_args(args)).await {
			Ok(_) => RespValue::simple_string("OK"),
			Err(e) => RespValue::error(format!("ERR {}", e)),
		}
	}
}

pub(super) fn pairs_from_args(args: &[Bytes]) -> Vec<(Bytes, Bytes)> {
	args.chunks_exact(2)
		.map(|pair| (pair[0].clone(), pair[1].clone()))
		.collect()
}
