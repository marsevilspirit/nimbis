use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

pub struct ZScoreCmd {
	meta: CmdMeta,
}

impl Default for ZScoreCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "ZSCORE".to_string(),
				arity: 3, // ZSCORE key member
			},
		}
	}
}

#[async_trait]
impl Cmd for ZScoreCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes]) -> RespValue {
		let key = args[0].clone();
		let member = args[1].clone();

		match storage.zscore(key, member).await {
			Ok(Some(score)) => {
				let score_str = score.to_string();
				RespValue::bulk_string(Bytes::copy_from_slice(score_str.as_bytes()))
			}
			Ok(None) => RespValue::null(),
			Err(e) => RespValue::error(e.to_string()),
		}
	}
}
