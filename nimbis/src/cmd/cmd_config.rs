use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;
use super::validate_arity;
use crate::config::SERVER_CONF;
use crate::config::ServerConfig;

pub(super) async fn execute(_storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
	let subcommand = &args[0];
	let (name, arity) = if subcommand.eq_ignore_ascii_case(b"GET") {
		("GET", 2)
	} else if subcommand.eq_ignore_ascii_case(b"SET") {
		("SET", 3)
	} else {
		return RespValue::error(format!(
			"ERR unknown CONFIG subcommand '{}'",
			String::from_utf8_lossy(subcommand).to_uppercase()
		));
	};

	if let Err(error) = validate_arity(name, arity, args.len()) {
		return RespValue::error(error);
	}

	match name {
		"GET" => get(&args[1..]),
		"SET" => set(&args[1..]),
		_ => unreachable!("CONFIG metadata and dispatch must stay in sync"),
	}
}

fn get(args: &[Bytes]) -> RespValue {
	let pattern = String::from_utf8_lossy(&args[0]);

	if pattern.contains('*') {
		let matched_fields = ServerConfig::match_fields(&pattern);

		if matched_fields.is_empty() {
			return RespValue::array(vec![]);
		}

		let config = SERVER_CONF.load();
		let mut result = Vec::new();
		for field_name in matched_fields {
			if let Ok(value) = config.get_field(field_name) {
				result.push(RespValue::bulk_string(Bytes::from(field_name.to_string())));
				result.push(RespValue::bulk_string(Bytes::from(value)));
			}
		}

		RespValue::array(result)
	} else {
		match SERVER_CONF.load().get_field(&pattern) {
			Ok(value) => RespValue::array(vec![
				RespValue::bulk_string(Bytes::from(pattern.into_owned())),
				RespValue::bulk_string(Bytes::from(value)),
			]),
			Err(error) => RespValue::error(error),
		}
	}
}

fn set(args: &[Bytes]) -> RespValue {
	let field_name = String::from_utf8_lossy(&args[0]);
	let value = String::from_utf8_lossy(&args[1]);
	let current = SERVER_CONF.load();
	let mut new_config = (**current).clone();

	match new_config.set_field(&field_name, &value) {
		Ok(_) => {
			SERVER_CONF.update(new_config);
			RespValue::simple_string("OK")
		}
		Err(error) => RespValue::error(error),
	}
}
