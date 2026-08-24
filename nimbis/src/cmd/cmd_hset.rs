use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;

pub struct HSetCmd {
	meta: CmdMeta,
}

impl Default for HSetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "HSET".to_string(),
				arity: -4, // HSET key field value [field value ...] -> min 3 args + command = 4
			},
		}
	}
}

#[async_trait]
impl Cmd for HSetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		// args: [key, field, value, field, value, ...]
		if args.len() < 3 || !(args.len() - 1).is_multiple_of(2) {
			return RespValue::error(
				"ERR wrong number of arguments for 'hset' command".to_string(),
			);
		}

		let (chunks, remainder) = args[1..].as_chunks::<2>();
		debug_assert!(remainder.is_empty());
		let fields = chunks
			.iter()
			.map(|[field, value]| (field.clone(), value.clone()))
			.collect();
		match storage.hset_many(args[0].clone(), fields).await {
			Ok(added_count) => RespValue::integer(added_count),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
