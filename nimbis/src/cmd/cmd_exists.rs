use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;
use super::typed_key_args::TypedKeysArgs;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let args = match TypedKeysArgs::parse(args) {
		Ok(args) => args,
		Err(error) => return RespValue::error(error),
	};

	match storage.exists_many(args.data_type(), args.keys()).await {
		Ok(count) => RespValue::Integer(count),
		Err(e) => RespValue::Error(Bytes::from(e.to_string())),
	}
}
