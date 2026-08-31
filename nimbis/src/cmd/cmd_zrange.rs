use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;

pub(super) async fn execute(storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let key = args[0].clone();

	// Helper closure to parse integer arguments
	let parse_int = |arg: &Bytes| -> Result<isize, RespValue> {
		String::from_utf8_lossy(arg)
			.parse::<isize>()
			.map_err(|_| RespValue::error("ERR value is not an integer or out of range"))
	};

	let start = match parse_int(&args[1]) {
		Ok(v) => v,
		Err(e) => return e,
	};

	let stop = match parse_int(&args[2]) {
		Ok(v) => v,
		Err(e) => return e,
	};

	let mut with_scores = false;
	if args.len() > 3 {
		let opt = String::from_utf8_lossy(&args[3]).to_uppercase();
		if opt == "WITHSCORES" {
			with_scores = true;
		} else {
			return RespValue::error("ERR syntax error");
		}
	}

	match storage.zrange(key, start, stop, with_scores).await {
		Ok(members) => RespValue::array(members.into_iter().map(RespValue::bulk_string)),
		Err(e) => RespValue::error(e.to_string()),
	}
}
