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

		let key = &args[0];
		let mut added_count = 0;

		let (chunks, remainder) = args[1..].as_chunks::<2>();
		debug_assert!(remainder.is_empty());
		for [field, value] in chunks {
			// TODO: Optimize by handling errors gracefully or transactional vs partial
			// success? Redis HSET is atomic per key. Here we do sequential updates.
			// If one fails, we return error.
			match storage
				.hset(key.clone(), field.clone(), value.clone())
				.await
			{
				Ok(count) => added_count += count,
				Err(e) => return RespValue::error(e.to_string()),
			}
		}

		RespValue::integer(added_count)
	}
}
