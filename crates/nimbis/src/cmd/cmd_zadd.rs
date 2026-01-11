use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct ZAddCmd {
	meta: CmdMeta,
}

impl Default for ZAddCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "ZADD".to_string(),
				arity: -4, // ZADD key score member [score member ...]
			},
		}
	}
}

#[async_trait]
impl Cmd for ZAddCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		// args: [key, score1, member1, score2, member2, ...]
		let key = args[0].clone();
		let remaining_args = &args[1..];

		if !remaining_args.len().is_multiple_of(2) {
			return RespValue::error("ERR syntax error");
		}

		let mut elements = Vec::with_capacity(remaining_args.len() / 2);
		for chunk in remaining_args.chunks_exact(2) {
			let score_str = String::from_utf8_lossy(&chunk[0]);
			let score = match score_str.parse::<f64>() {
				Ok(s) => s,
				Err(_) => return RespValue::error("ERR value is not a valid float"),
			};
			if score.is_nan() {
				return RespValue::error("ERR resulting score is not a number (NaN)");
			}

			let member = chunk[1].clone();
			elements.push((score, member));
		}

		match storage.zadd(key, elements).await {
			Ok(added) => RespValue::integer(added as i64),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
