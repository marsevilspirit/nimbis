use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use super::utils::parse_data_type;

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
		let data_type = match parse_data_type(&args[0]) {
			Ok(data_type) => data_type,
			Err(error) => return RespValue::error(error),
		};

		match storage
			.exists_many(data_type, args[1..].iter().cloned())
			.await
		{
			Ok(count) => RespValue::Integer(count),
			Err(e) => RespValue::Error(Bytes::from(e.to_string())),
		}
	}
}
