use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	// args: [key, score1, member1, score2, member2, ...]
	let key = args[0].clone();
	let remaining_args = &args[1..];

	if !remaining_args.len().is_multiple_of(2) {
		return RespValue::error("ERR syntax error");
	}

	let mut elements = Vec::with_capacity(remaining_args.len() / 2);
	let (chunks, remainder) = remaining_args.as_chunks::<2>();
	debug_assert!(remainder.is_empty());
	for [score_bytes, member] in chunks {
		let score_str = String::from_utf8_lossy(score_bytes);
		let score = match score_str.parse::<f64>() {
			Ok(s) => s,
			Err(_) => return RespValue::error("ERR value is not a valid float"),
		};
		if score.is_nan() {
			return RespValue::error("ERR resulting score is not a number (NaN)");
		}

		elements.push((score, member.clone()));
	}

	match storage.zadd(key, elements).await {
		Ok(added) => RespValue::integer(added as i64),
		Err(e) => RespValue::error(e.to_string()),
	}
}
