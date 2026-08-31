use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	// args: [key, field, value, field, value, ...]
	if args.len() < 3 || !(args.len() - 1).is_multiple_of(2) {
		return RespValue::error("ERR wrong number of arguments for 'hset' command".to_string());
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
