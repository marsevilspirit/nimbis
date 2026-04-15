use std::collections::HashMap;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdMeta;

/// HELLO command implementation
pub struct HelloCmd {
	meta: CmdMeta,
}

impl Default for HelloCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "HELLO".to_string(),
				arity: -1, // HELLO [protover [AUTH username password] [SETNAME clientname]]
			},
		}
	}
}

impl HelloCmd {
	fn parse_proto(args: &[bytes::Bytes]) -> Result<i64, RespValue> {
		if args.is_empty() {
			return Ok(2);
		}

		if args.len() > 1 {
			return Err(RespValue::error("ERR syntax error"));
		}

		match std::str::from_utf8(&args[0]) {
			Ok("2") => Ok(2),
			Ok("3") => Ok(3),
			_ => Err(RespValue::error("NOPROTO unsupported protocol version")),
		}
	}

	fn resp2_hello(proto: i64) -> RespValue {
		RespValue::array(vec![
			RespValue::bulk_string("server"),
			RespValue::bulk_string("nimbis"),
			RespValue::bulk_string("version"),
			RespValue::bulk_string(env!("CARGO_PKG_VERSION")),
			RespValue::bulk_string("proto"),
			RespValue::integer(proto),
			RespValue::bulk_string("id"),
			RespValue::integer(1),
			RespValue::bulk_string("mode"),
			RespValue::bulk_string("standalone"),
			RespValue::bulk_string("role"),
			RespValue::bulk_string("master"),
			RespValue::bulk_string("modules"),
			RespValue::array(Vec::<RespValue>::new()),
		])
	}

	fn resp3_hello(proto: i64) -> RespValue {
		let mut map = HashMap::new();
		map.insert(
			RespValue::bulk_string(Bytes::from_static(b"server")),
			RespValue::bulk_string(Bytes::from_static(b"nimbis")),
		);
		map.insert(
			RespValue::bulk_string(Bytes::from_static(b"version")),
			RespValue::bulk_string(Bytes::from_static(env!("CARGO_PKG_VERSION").as_bytes())),
		);
		map.insert(
			RespValue::bulk_string(Bytes::from_static(b"proto")),
			RespValue::integer(proto),
		);
		map.insert(
			RespValue::bulk_string(Bytes::from_static(b"id")),
			RespValue::integer(1),
		);
		map.insert(
			RespValue::bulk_string(Bytes::from_static(b"mode")),
			RespValue::bulk_string(Bytes::from_static(b"standalone")),
		);
		map.insert(
			RespValue::bulk_string(Bytes::from_static(b"role")),
			RespValue::bulk_string(Bytes::from_static(b"master")),
		);
		map.insert(
			RespValue::bulk_string(Bytes::from_static(b"modules")),
			RespValue::array(Vec::<RespValue>::new()),
		);
		RespValue::Map(map)
	}
}

#[async_trait]
impl Cmd for HelloCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, args: &[bytes::Bytes]) -> RespValue {
		let proto = match Self::parse_proto(args) {
			Ok(proto) => proto,
			Err(err) => return err,
		};

		match proto {
			2 => Self::resp2_hello(proto),
			3 => Self::resp3_hello(proto),
			_ => RespValue::error("NOPROTO unsupported protocol version"),
		}
	}
}
