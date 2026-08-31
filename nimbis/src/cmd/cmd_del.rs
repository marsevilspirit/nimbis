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

	match storage.del(args.data_type(), args.keys()).await {
		Ok(deleted) => RespValue::Integer(deleted),
		Err(e) => RespValue::error(e.to_string()),
	}
}
