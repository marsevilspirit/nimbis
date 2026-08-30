use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = args[0].clone();

	match storage.smembers(key).await {
		Ok(members) => {
			let resp_members: Vec<RespValue> =
				members.into_iter().map(RespValue::bulk_string).collect();
			RespValue::Array(resp_members)
		}
		Err(e) => RespValue::error(e.to_string()),
	}
}
