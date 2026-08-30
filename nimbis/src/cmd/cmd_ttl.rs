use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;
use super::typed_key_args::TypedKeyArgs;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let args = match TypedKeyArgs::parse(args) {
		Ok(args) => args,
		Err(error) => return RespValue::error(error),
	};
	let (data_type, key) = args.into_parts();
	match storage.ttl(data_type, key).await {
		Ok(Some(ttl_ms)) => RespValue::Integer(match ttl_ms {
			-1 => -1,
			_ => ttl_ms / 1000,
		}),
		Ok(None) => RespValue::Integer(-2), // Key does not exist
		Err(e) => RespValue::Error(Bytes::from(e.to_string())),
	}
}
