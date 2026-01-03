use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

#[derive(Debug, Clone)]
pub struct ExpireCmd {
	meta: CmdMeta,
}

impl Default for ExpireCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "EXPIRE".to_string(),
				arity: 3, // EXPIRE key seconds
			},
		}
	}
}

#[async_trait]
impl Cmd for ExpireCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		let seconds_str = String::from_utf8_lossy(&args[1]);
		let seconds = match seconds_str.parse::<u64>() {
			Ok(s) => s,
			Err(_) => {
				return RespValue::Error(Bytes::from(
					"ERR value is not an integer or out of range",
				));
			}
		};

		let now = chrono::Utc::now().timestamp_millis() as u64;

		let expire_time = now + seconds * 1000;

		match storage.expire(key, expire_time).await {
			Ok(true) => RespValue::Integer(1),
			Ok(false) => RespValue::Integer(0),
			Err(e) => RespValue::Error(Bytes::from(e.to_string())),
		}
	}
}
