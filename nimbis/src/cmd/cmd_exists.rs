use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::typed_key_args::TypedKeysArgs;

pub struct ExistsCmd {
	meta: CmdMeta,
}

impl Default for ExistsCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "EXISTS".to_string(),
				arity: -3, // EXISTS type key [key ...]
			},
		}
	}
}

#[async_trait]
impl Cmd for ExistsCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let args = match TypedKeysArgs::parse(args) {
			Ok(args) => args,
			Err(error) => return RespValue::error(error),
		};

		match storage.exists_many(args.data_type(), args.keys()).await {
			Ok(count) => RespValue::Integer(count),
			Err(e) => RespValue::Error(Bytes::from(e.to_string())),
		}
	}
}
