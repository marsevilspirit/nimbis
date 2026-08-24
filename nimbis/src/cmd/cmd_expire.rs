use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::utils::parse_data_type;
use super::utils::parse_int;

#[derive(Debug, Clone)]
pub struct ExpireCmd {
	meta: CmdMeta,
}

impl Default for ExpireCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "EXPIRE".to_string(),
				arity: 4, // EXPIRE type key seconds
			},
		}
	}
}

#[async_trait]
impl Cmd for ExpireCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let data_type = match parse_data_type(&args[0]) {
			Ok(data_type) => data_type,
			Err(error) => return RespValue::error(error),
		};
		let key = args[1].clone();
		let seconds = match parse_int::<u64>(&args[2]) {
			Ok(s) => s,
			Err(error) => return RespValue::error(error),
		};

		let now = match u64::try_from(chrono::Utc::now().timestamp_millis()) {
			Ok(now) => now,
			Err(_) => {
				return RespValue::error("ERR value is not an integer or out of range");
			}
		};
		let expire_time = match seconds
			.checked_mul(1000)
			.and_then(|duration| now.checked_add(duration))
		{
			Some(expire_time) => expire_time,
			None => return RespValue::error("ERR value is not an integer or out of range"),
		};

		match storage.expire(data_type, key, expire_time).await {
			Ok(true) => RespValue::Integer(1),
			Ok(false) => RespValue::Integer(0),
			Err(e) => RespValue::Error(Bytes::from(e.to_string())),
		}
	}
}
