use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::RoutingPolicy;
use crate::coordinator::CommandPlan;
use crate::coordinator::CoordinatedCommandPlan;
use crate::coordinator::LockedExecution;

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

	fn plan(&self, args: &[Bytes], _worker_count: usize) -> Result<CommandPlan, RespValue> {
		if !args.len().is_multiple_of(2) {
			return Err(RespValue::error(
				"ERR wrong number of arguments for 'mset' command",
			));
		}

		let pairs = pairs_from_args(args);
		let keys = pairs.iter().map(|(key, _)| key.clone()).collect();
		Ok(CoordinatedCommandPlan::LockedMultiKey {
			keys,
			execution: LockedExecution::MSet { pairs },
		}
		.into())
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		if !args.len().is_multiple_of(2) {
			return RespValue::error("ERR wrong number of arguments for 'mset' command");
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

#[cfg(test)]
mod tests {
	use bytes::Bytes;

	use super::MSetCmd;
	use crate::cmd::Cmd;
	use crate::coordinator::CommandPlan;
	use crate::coordinator::CoordinatedCommandPlan;
	use crate::coordinator::LockedExecution;

	#[test]
	fn mset_plan_uses_locked_multi_key_execution() {
		let cmd = MSetCmd::default();
		let args = vec![
			Bytes::from_static(b"k1"),
			Bytes::from_static(b"v1"),
			Bytes::from_static(b"k2"),
			Bytes::from_static(b"v2"),
		];

		let plan = cmd.plan(&args, 2).expect("mset plan");

		match plan {
			CommandPlan::Coordinated(CoordinatedCommandPlan::LockedMultiKey {
				keys,
				execution: LockedExecution::MSet { pairs },
			}) => {
				assert_eq!(
					keys,
					vec![Bytes::from_static(b"k1"), Bytes::from_static(b"k2")]
				);
				assert_eq!(
					pairs,
					vec![
						(Bytes::from_static(b"k1"), Bytes::from_static(b"v1")),
						(Bytes::from_static(b"k2"), Bytes::from_static(b"v2")),
					]
				);
			}
			other => panic!("unexpected MSET plan: {:?}", other),
		}
	}
}
