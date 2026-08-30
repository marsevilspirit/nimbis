use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
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
