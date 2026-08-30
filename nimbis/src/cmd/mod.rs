use std::sync::Arc;

use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use crate::client::ClientSessions;

const COMMAND_NAME_STACK_CAPACITY: usize = 32;

#[derive(Debug, Clone)]
pub struct CmdContext {
	pub client_id: i64,
	pub client_sessions: Arc<ClientSessions>,
}

pub async fn execute(
	name: &str,
	storage: &Storage,
	args: &[Bytes],
	ctx: &CmdContext,
) -> Option<RespValue> {
	let mut uppercase = [0; COMMAND_NAME_STACK_CAPACITY];
	let name = normalize_command_name(name, &mut uppercase)?;
	let arg_count = args.len() + 1;
	macro_rules! command {
		($name:literal, $arity:literal, $module:ident) => {{
			if let Err(error) = validate_arity($name, $arity, arg_count) {
				RespValue::error(error)
			} else {
				$module::execute(storage, args, ctx).await
			}
		}};
	}

	Some(match name {
		"APPEND" => command!("APPEND", 3, cmd_append),
		"CLIENT" => command!("CLIENT", -2, cmd_client),
		"CONFIG" => command!("CONFIG", -3, cmd_config),
		"DECR" => command!("DECR", 2, cmd_decr),
		"DEL" => command!("DEL", -3, cmd_del),
		"EXISTS" => command!("EXISTS", -3, cmd_exists),
		"EXPIRE" => command!("EXPIRE", 4, cmd_expire),
		"FLUSHDB" => command!("FLUSHDB", 0, cmd_flushdb),
		"GET" => command!("GET", 2, cmd_get),
		"HDEL" => command!("HDEL", -3, cmd_hdel),
		"HELLO" => command!("HELLO", -1, cmd_hello),
		"HGET" => command!("HGET", 3, cmd_hget),
		"HGETALL" => command!("HGETALL", 2, cmd_hgetall),
		"HLEN" => command!("HLEN", 2, cmd_hlen),
		"HMGET" => command!("HMGET", -3, cmd_hmget),
		"HSET" => command!("HSET", -4, cmd_hset),
		"INCR" => command!("INCR", 2, cmd_incr),
		"LLEN" => command!("LLEN", 2, cmd_llen),
		"LPOP" => command!("LPOP", -2, cmd_lpop),
		"LPUSH" => command!("LPUSH", -3, cmd_lpush),
		"LRANGE" => command!("LRANGE", 4, cmd_lrange),
		"PING" => command!("PING", -1, cmd_ping),
		"RPOP" => command!("RPOP", -2, cmd_rpop),
		"RPUSH" => command!("RPUSH", -3, cmd_rpush),
		"SADD" => command!("SADD", -3, cmd_sadd),
		"SCARD" => command!("SCARD", 2, cmd_scard),
		"SET" => command!("SET", 3, cmd_set),
		"SISMEMBER" => command!("SISMEMBER", 3, cmd_sismember),
		"SMEMBERS" => command!("SMEMBERS", 2, cmd_smembers),
		"SREM" => command!("SREM", -3, cmd_srem),
		"TTL" => command!("TTL", 3, cmd_ttl),
		"ZADD" => command!("ZADD", -4, cmd_zadd),
		"ZCARD" => command!("ZCARD", 2, cmd_zcard),
		"ZRANGE" => command!("ZRANGE", -4, cmd_zrange),
		"ZREM" => command!("ZREM", -3, cmd_zrem),
		"ZSCORE" => command!("ZSCORE", 3, cmd_zscore),
		_ => return None,
	})
}

fn normalize_command_name<'a>(
	name: &str,
	uppercase: &'a mut [u8; COMMAND_NAME_STACK_CAPACITY],
) -> Option<&'a str> {
	if name.len() > COMMAND_NAME_STACK_CAPACITY {
		return None;
	}

	uppercase[..name.len()].copy_from_slice(name.as_bytes());
	uppercase[..name.len()].make_ascii_uppercase();
	Some(std::str::from_utf8(&uppercase[..name.len()]).expect("ASCII uppercasing preserves UTF-8"))
}

fn validate_arity(name: &str, arity: i16, arg_count: usize) -> Result<(), String> {
	let valid = match arity {
		0 => true,
		arity if arity > 0 => arg_count == arity as usize,
		arity => arg_count >= (-arity) as usize,
	};

	if valid {
		Ok(())
	} else {
		Err(format!(
			"ERR wrong number of arguments for '{}' command",
			name.to_lowercase()
		))
	}
}

pub struct ParsedCmd {
	name: Bytes,
	pub args: Vec<Bytes>,
}

impl ParsedCmd {
	pub fn name(&self) -> &str {
		std::str::from_utf8(&self.name).expect("command name was validated while parsing")
	}
}

impl TryFrom<RespValue> for ParsedCmd {
	type Error = String;

	fn try_from(value: RespValue) -> Result<Self, Self::Error> {
		let args = value.as_array().ok_or("Expected array")?;

		if args.is_empty() {
			return Err("Empty command".to_string());
		}

		let cmd_name = args[0].as_bytes().ok_or("Invalid command type")?;
		std::str::from_utf8(cmd_name).map_err(|_| "Invalid command type")?;

		let cmd_args: Result<Vec<Bytes>, _> = args[1..]
			.iter()
			.map(|v| v.as_bytes().cloned().ok_or("Invalid argument"))
			.collect();

		Ok(ParsedCmd {
			name: cmd_name.clone(),
			args: cmd_args?,
		})
	}
}

mod cmd_append;
mod cmd_client;
mod cmd_config;
mod cmd_decr;
mod cmd_del;
mod cmd_exists;
mod cmd_expire;
mod cmd_flushdb;
mod cmd_get;
mod cmd_hdel;
mod cmd_hello;
mod cmd_hget;
mod cmd_hgetall;
mod cmd_hlen;
mod cmd_hmget;
mod cmd_hset;
mod cmd_incr;
mod cmd_llen;
mod cmd_lpop;
mod cmd_lpush;
mod cmd_lrange;
mod cmd_ping;
mod cmd_rpop;
mod cmd_rpush;
mod cmd_sadd;
mod cmd_scard;
mod cmd_set;
mod cmd_sismember;
mod cmd_smembers;
mod cmd_srem;
mod cmd_ttl;
mod cmd_zadd;
mod cmd_zcard;
mod cmd_zrange;
mod cmd_zrem;
mod cmd_zscore;
mod typed_key_args;
mod utils;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn command_lookup_is_case_insensitive() {
		for name in ["PING", "ping", "pInG"] {
			let mut uppercase = [0; COMMAND_NAME_STACK_CAPACITY];
			assert_eq!(normalize_command_name(name, &mut uppercase), Some("PING"));
		}
	}

	#[test]
	fn long_unknown_command_is_not_found() {
		let name = "x".repeat(COMMAND_NAME_STACK_CAPACITY + 1);
		let mut uppercase = [0; COMMAND_NAME_STACK_CAPACITY];

		assert_eq!(normalize_command_name(&name, &mut uppercase), None);
	}

	#[test]
	fn preserves_command_name_case() {
		let value = RespValue::array([RespValue::bulk_string("pInG")]);

		let parsed = ParsedCmd::try_from(value).unwrap();

		assert_eq!(parsed.name(), "pInG");
	}

	#[test]
	fn rejects_non_utf8_command_name() {
		let value = RespValue::array([RespValue::bulk_string(Bytes::from_static(b"\xff"))]);

		let error = ParsedCmd::try_from(value).err().unwrap();

		assert_eq!(error, "Invalid command type");
	}
}
