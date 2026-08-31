use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;
use nimbis_storage::StorageError;

use super::CmdContext;
use super::typed_key_args::TypedKeyArgs;
use super::utils::parse_int;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let typed_key = match TypedKeyArgs::parse(args) {
		Ok(typed_key) => typed_key,
		Err(error) => return RespValue::error(error),
	};
	let (data_type, key) = typed_key.into_parts();
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
		Err(StorageError::InvalidExpiration { .. }) => {
			RespValue::error("ERR value is not an integer or out of range")
		}
		Err(e) => RespValue::Error(Bytes::from(e.to_string())),
	}
}
