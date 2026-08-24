use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::typed_key_args::TypedKeysArgs;

pub struct DelCmd {
	meta: CmdMeta,
}

impl Default for DelCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "DEL".to_string(),
				arity: -3, // DEL type key [key ...]
			},
		}
	}
}

#[async_trait]
impl Cmd for DelCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let args = match TypedKeysArgs::parse(args) {
			Ok(args) => args,
			Err(error) => return RespValue::error(error),
		};

		match storage.del(args.data_type(), args.keys()).await {
			Ok(deleted) => RespValue::Integer(deleted),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
