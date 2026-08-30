use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::CmdContext;
use super::validate_arity;

pub(super) async fn execute(_storage: &Storage, args: &[Bytes], ctx: &CmdContext) -> RespValue {
	let subcommand = &args[0];
	let (name, arity) = if subcommand.eq_ignore_ascii_case(b"ID") {
		("ID", 1)
	} else if subcommand.eq_ignore_ascii_case(b"SETNAME") {
		("SETNAME", 2)
	} else if subcommand.eq_ignore_ascii_case(b"GETNAME") {
		("GETNAME", 1)
	} else if subcommand.eq_ignore_ascii_case(b"LIST") {
		("LIST", 1)
	} else {
		return RespValue::error(format!(
			"ERR unknown CLIENT subcommand '{}'",
			String::from_utf8_lossy(subcommand).to_uppercase()
		));
	};

	if let Err(error) = validate_arity(name, arity, args.len()) {
		return RespValue::error(error);
	}

	match name {
		"ID" => RespValue::integer(ctx.client_id),
		"SETNAME" => set_name(&args[1..], ctx),
		"GETNAME" => get_name(ctx),
		"LIST" => list(ctx),
		_ => unreachable!("CLIENT metadata and dispatch must stay in sync"),
	}
}

fn set_name(args: &[Bytes], ctx: &CmdContext) -> RespValue {
	if ctx.client_sessions.set_name(ctx.client_id, args[0].clone()) {
		RespValue::simple_string("OK")
	} else {
		RespValue::error("ERR client not found")
	}
}

fn get_name(ctx: &CmdContext) -> RespValue {
	match ctx.client_sessions.get_name(ctx.client_id) {
		Some(name) => RespValue::bulk_string(name),
		None => RespValue::null(),
	}
}

fn list(ctx: &CmdContext) -> RespValue {
	let lines = ctx
		.client_sessions
		.list()
		.into_iter()
		.map(|(client_id, name)| {
			let name = name
				.map(|value| String::from_utf8_lossy(&value).into_owned())
				.unwrap_or_default();
			format!("id={} name={}", client_id, name)
		})
		.collect::<Vec<_>>()
		.join("\n");

	RespValue::bulk_string(Bytes::from(lines))
}
