use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::CommandKind;
use super::KeySpec;

pub struct MSetCmd {
	meta: CmdMeta,
}

impl Default for MSetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "MSET".to_string(),
				arity: -3,
				key_spec: KeySpec::Step { first: 0, step: 2 },
				kind: CommandKind::Write,
			},
		}
	}
}

#[async_trait]
impl Cmd for MSetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
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

	#[test]
	fn mset_plan_routes_same_worker_multi_key_write_to_owner() {
		let cmd = MSetCmd::default();
		let args = vec![
			Bytes::from_static(b"k1"),
			Bytes::from_static(b"v1"),
			Bytes::from_static(b"k2"),
			Bytes::from_static(b"v2"),
		];

		let plan = cmd.plan(&args, 1).expect("mset plan");

		match plan {
			CommandPlan::Coordinated(CoordinatedCommandPlan::SingleKey { key, request }) => {
				assert_eq!(key, Bytes::from_static(b"k1"));
				assert_eq!(request.name, "MSET");
				assert_eq!(request.args, args);
			}
			other => panic!("unexpected MSET plan: {:?}", other),
		}
	}

	#[test]
	fn mset_plan_rejects_odd_argument_count() {
		let cmd = MSetCmd::default();
		let args = vec![
			Bytes::from_static(b"k1"),
			Bytes::from_static(b"v1"),
			Bytes::from_static(b"k2"),
		];

		assert!(cmd.plan(&args, 1).is_err());
	}
}
