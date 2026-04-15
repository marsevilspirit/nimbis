use std::collections::HashMap;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdContext;
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
				arity: -1, // HELLO [protover]
			},
		}
	}
}

impl HelloCmd {
	fn parse_proto(args: &[Bytes]) -> Result<i64, RespValue> {
		if args.is_empty() {
			return Ok(2);
		}

		if args.len() > 1 {
			return Err(RespValue::error(
				"ERR HELLO expects at most one argument (protocol version)",
			));
		}

		match std::str::from_utf8(&args[0]) {
			Ok("2") => Ok(2),
			Ok("3") => Ok(3),
			_ => Err(RespValue::error(
				"NOPROTO unsupported protocol version. Use 2 or 3",
			)),
		}
	}

	fn resp2_hello(proto: i64, client_id: i64) -> RespValue {
		RespValue::array(vec![
			RespValue::bulk_string("server"),
			RespValue::bulk_string("nimbis"),
			RespValue::bulk_string("version"),
			RespValue::bulk_string(env!("CARGO_PKG_VERSION")),
			RespValue::bulk_string("proto"),
			RespValue::integer(proto),
			RespValue::bulk_string("id"),
			RespValue::integer(client_id),
			RespValue::bulk_string("mode"),
			RespValue::bulk_string("standalone"),
			RespValue::bulk_string("role"),
			RespValue::bulk_string("master"),
			RespValue::bulk_string("modules"),
			RespValue::array(Vec::<RespValue>::new()),
		])
	}

	fn resp3_hello(proto: i64, client_id: i64) -> RespValue {
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
			RespValue::integer(client_id),
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

	async fn do_cmd(&self, _storage: &Storage, args: &[Bytes], ctx: &CmdContext) -> RespValue {
		let proto = match Self::parse_proto(args) {
			Ok(proto) => proto,
			Err(err) => return err,
		};

		if proto == 2 {
			Self::resp2_hello(proto, ctx.client_id)
		} else {
			Self::resp3_hello(proto, ctx.client_id)
		}
	}
}

#[cfg(test)]
mod tests {
	use resp::RespValue;

	use super::Bytes;
	use super::HelloCmd;

	#[test]
	fn test_parse_proto_default_to_resp2() {
		let proto = HelloCmd::parse_proto(&[]).expect("parse proto should succeed");
		assert_eq!(proto, 2);
	}

	#[test]
	fn test_parse_proto_resp2() {
		let proto =
			HelloCmd::parse_proto(&[Bytes::from_static(b"2")]).expect("parse proto should succeed");
		assert_eq!(proto, 2);
	}

	#[test]
	fn test_parse_proto_resp3() {
		let proto =
			HelloCmd::parse_proto(&[Bytes::from_static(b"3")]).expect("parse proto should succeed");
		assert_eq!(proto, 3);
	}

	#[test]
	fn test_parse_proto_rejects_invalid_version() {
		let err = HelloCmd::parse_proto(&[Bytes::from_static(b"4")]).expect_err("should error");
		assert_eq!(
			err,
			RespValue::error("NOPROTO unsupported protocol version. Use 2 or 3")
		);
	}

	#[test]
	fn test_parse_proto_rejects_too_many_arguments() {
		let err =
			HelloCmd::parse_proto(&[Bytes::from_static(b"3"), Bytes::from_static(b"SETNAME")])
				.expect_err("should error");
		assert_eq!(
			err,
			RespValue::error("ERR HELLO expects at most one argument (protocol version)")
		);
	}

	#[test]
	fn test_parse_proto_rejects_non_utf8_input() {
		let err = HelloCmd::parse_proto(&[Bytes::from_static(&[0xFF])]).expect_err("should error");
		assert_eq!(
			err,
			RespValue::error("NOPROTO unsupported protocol version. Use 2 or 3")
		);
	}

	#[test]
	fn test_resp2_hello_structure() {
		let resp = HelloCmd::resp2_hello(2, 42);
		let arr = resp.as_array().expect("HELLO 2 should return RESP2 array");
		assert_eq!(arr.len(), 14);
		assert_eq!(arr[0], RespValue::bulk_string("server"));
		assert_eq!(arr[1], RespValue::bulk_string("nimbis"));
		assert_eq!(arr[4], RespValue::bulk_string("proto"));
		assert_eq!(arr[5], RespValue::integer(2));
		assert_eq!(arr[7], RespValue::integer(42));
	}

	#[test]
	fn test_resp3_hello_contains_proto() {
		let resp = HelloCmd::resp3_hello(3, 7);
		let map = resp.as_map().expect("HELLO 3 should return RESP3 map");
		assert_eq!(
			map.get(&RespValue::bulk_string("proto")),
			Some(&RespValue::integer(3))
		);
		assert_eq!(
			map.get(&RespValue::bulk_string("id")),
			Some(&RespValue::integer(7))
		);
		assert_eq!(
			map.get(&RespValue::bulk_string("server")),
			Some(&RespValue::bulk_string("nimbis"))
		);
	}
}
