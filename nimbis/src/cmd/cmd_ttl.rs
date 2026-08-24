use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::typed_key_args::TypedKeyArgs;

#[derive(Debug, Clone)]
pub struct TtlCmd {
	meta: CmdMeta,
}

impl Default for TtlCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "TTL".to_string(),
				arity: 3, // TTL type key
			},
		}
	}
}

#[async_trait]
impl Cmd for TtlCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
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
}
