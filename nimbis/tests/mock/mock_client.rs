use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
use std::net::TcpStream;
use std::time::Duration;

use bytes::Bytes;
use nimbis_resp::RespEncoder;
use nimbis_resp::RespParseResult;
use nimbis_resp::RespParser;
use nimbis_resp::RespValue;

use crate::mock::utils::resp_to_strings;

pub struct MockNimbisClient {
	id: i64,
	stream: TcpStream,
	parser: RespParser,
}

impl MockNimbisClient {
	pub fn connect(host: &str, port: u16) -> std::io::Result<Self> {
		let stream = TcpStream::connect((host, port))?;
		configure_stream(&stream)?;
		let mut client = Self {
			id: 0,
			stream,
			parser: RespParser::new(),
		};
		client.id = client.client_id();
		Ok(client)
	}

	pub fn id(&self) -> i64 {
		self.id
	}

	pub fn execute(&mut self, args: &[&str]) -> RespValue {
		let req = RespValue::array(
			args.iter()
				.map(|arg| RespValue::bulk_string(Bytes::copy_from_slice(arg.as_bytes()))),
		);
		self.stream
			.write_all(&req.encode().expect("encode request"))
			.unwrap_or_else(|e| panic!("write request {:?}: {}", args, e));
		self.read_response()
			.unwrap_or_else(|e| panic!("read response for {:?}: {}", args, e))
	}

	fn read_response(&mut self) -> Result<RespValue, String> {
		let mut buf = Vec::with_capacity(4096);
		let mut read_chunk = [0u8; 1024];

		loop {
			let n = self
				.stream
				.read(&mut read_chunk)
				.map_err(|e| e.to_string())?;
			if n == 0 {
				return Err("connection closed before full response".into());
			}

			buf.extend_from_slice(&read_chunk[..n]);
			let mut bytes = bytes::BytesMut::from(buf.as_slice());
			match self.parser.parse(&mut bytes) {
				RespParseResult::Complete(v) => return Ok(v),
				RespParseResult::Incomplete => {}
				RespParseResult::Error(e) => return Err(format!("RESP parse error: {}", e)),
			}
		}
	}

	pub fn ping(&mut self) -> String {
		self.execute(&["PING"])
			.to_string_lossy()
			.expect("unexpected ping response")
	}

	pub fn set(&mut self, key: &str, value: &str) -> String {
		self.execute(&["SET", key, value])
			.to_string_lossy()
			.expect("unexpected set response")
	}

	pub fn get(&mut self, key: &str) -> String {
		match self.execute(&["GET", key]) {
			RespValue::Null => String::new(),
			resp => resp.to_string_lossy().expect("unexpected get response"),
		}
	}

	pub fn mget(&mut self, keys: &[&str]) -> Vec<String> {
		let mut args = vec!["MGET"];
		args.extend_from_slice(keys);
		resp_to_strings(self.execute(&args))
	}

	pub fn mset(&mut self, pairs: &[(&str, &str)]) -> String {
		let mut args = vec!["MSET"];
		for (key, value) in pairs {
			args.extend([*key, *value]);
		}
		self.execute(&args)
			.to_string_lossy()
			.expect("unexpected MSET response")
	}

	pub fn msetnx(&mut self, pairs: &[(&str, &str)]) -> i64 {
		let mut args = vec!["MSETNX"];
		for (key, value) in pairs {
			args.extend([*key, *value]);
		}
		self.execute(&args)
			.as_integer()
			.expect("MSETNX should return integer")
	}

	#[allow(dead_code)]
	pub fn flushdb(&mut self) -> bool {
		matches!(
			self.execute(&["FLUSHDB"]),
			RespValue::SimpleString(s) if s.as_ref() == b"OK"
		)
	}

	// -- string commands --

	pub fn del(&mut self, keys: &[&str]) -> i64 {
		let mut args = vec!["DEL"];
		args.extend_from_slice(keys);
		self.execute(&args)
			.as_integer()
			.expect("DEL should return integer")
	}

	pub fn exists(&mut self, key: &str) -> bool {
		self.execute(&["EXISTS", key])
			.as_integer()
			.expect("EXISTS should return integer")
			== 1
	}

	pub fn exists_count(&mut self, keys: &[&str]) -> i64 {
		let mut args = vec!["EXISTS"];
		args.extend_from_slice(keys);
		self.execute(&args)
			.as_integer()
			.expect("EXISTS should return integer")
	}

	pub fn incr(&mut self, key: &str) -> i64 {
		self.execute(&["INCR", key])
			.as_integer()
			.expect("INCR should return integer")
	}

	pub fn decr(&mut self, key: &str) -> i64 {
		self.execute(&["DECR", key])
			.as_integer()
			.expect("DECR should return integer")
	}

	pub fn append(&mut self, key: &str, value: &str) -> i64 {
		self.execute(&["APPEND", key, value])
			.as_integer()
			.expect("APPEND should return integer")
	}

	// -- hash commands --

	pub fn hset(&mut self, key: &str, field: &str, value: &str) -> i64 {
		self.execute(&["HSET", key, field, value])
			.as_integer()
			.expect("HSET should return integer")
	}

	pub fn hget(&mut self, key: &str, field: &str) -> String {
		match self.execute(&["HGET", key, field]) {
			RespValue::Null => String::new(),
			resp => resp.to_string_lossy().expect("unexpected HGET response"),
		}
	}

	pub fn hdel(&mut self, key: &str, field: &str) -> i64 {
		self.execute(&["HDEL", key, field])
			.as_integer()
			.expect("HDEL should return integer")
	}

	pub fn hlen(&mut self, key: &str) -> i64 {
		self.execute(&["HLEN", key])
			.as_integer()
			.expect("HLEN should return integer")
	}

	pub fn hgetall(&mut self, key: &str) -> Vec<String> {
		resp_to_strings(self.execute(&["HGETALL", key]))
	}

	pub fn hmget(&mut self, key: &str, fields: &[&str]) -> Vec<String> {
		let mut args = vec!["HMGET", key];
		args.extend_from_slice(fields);
		resp_to_strings(self.execute(&args))
	}

	// -- list commands --

	pub fn lpush(&mut self, key: &str, elements: &[&str]) -> i64 {
		let mut args = vec!["LPUSH", key];
		args.extend_from_slice(elements);
		self.execute(&args)
			.as_integer()
			.expect("LPUSH should return integer")
	}

	pub fn rpush(&mut self, key: &str, elements: &[&str]) -> i64 {
		let mut args = vec!["RPUSH", key];
		args.extend_from_slice(elements);
		self.execute(&args)
			.as_integer()
			.expect("RPUSH should return integer")
	}

	pub fn lpop(&mut self, key: &str) -> String {
		match self.execute(&["LPOP", key]) {
			RespValue::Null => String::new(),
			resp => resp.to_string_lossy().expect("unexpected LPOP response"),
		}
	}

	pub fn rpop(&mut self, key: &str) -> String {
		match self.execute(&["RPOP", key]) {
			RespValue::Null => String::new(),
			resp => resp.to_string_lossy().expect("unexpected RPOP response"),
		}
	}

	pub fn llen(&mut self, key: &str) -> i64 {
		self.execute(&["LLEN", key])
			.as_integer()
			.expect("LLEN should return integer")
	}

	pub fn lrange(&mut self, key: &str, start: i64, stop: i64) -> Vec<String> {
		let start_s = start.to_string();
		let stop_s = stop.to_string();
		resp_to_strings(self.execute(&["LRANGE", key, &start_s, &stop_s]))
	}

	// -- set commands --

	pub fn sadd(&mut self, key: &str, members: &[&str]) -> i64 {
		let mut args = vec!["SADD", key];
		args.extend_from_slice(members);
		self.execute(&args)
			.as_integer()
			.expect("SADD should return integer")
	}

	pub fn smembers(&mut self, key: &str) -> Vec<String> {
		resp_to_strings(self.execute(&["SMEMBERS", key]))
	}

	pub fn sismember(&mut self, key: &str, member: &str) -> bool {
		self.execute(&["SISMEMBER", key, member])
			.as_integer()
			.expect("SISMEMBER should return integer")
			== 1
	}

	pub fn srem(&mut self, key: &str, members: &[&str]) -> i64 {
		let mut args = vec!["SREM", key];
		args.extend_from_slice(members);
		self.execute(&args)
			.as_integer()
			.expect("SREM should return integer")
	}

	pub fn scard(&mut self, key: &str) -> i64 {
		self.execute(&["SCARD", key])
			.as_integer()
			.expect("SCARD should return integer")
	}

	pub fn sunion(&mut self, keys: &[&str]) -> Vec<String> {
		let mut args = vec!["SUNION"];
		args.extend_from_slice(keys);
		resp_to_strings(self.execute(&args))
	}

	pub fn sinter(&mut self, keys: &[&str]) -> Vec<String> {
		let mut args = vec!["SINTER"];
		args.extend_from_slice(keys);
		resp_to_strings(self.execute(&args))
	}

	pub fn sdiff(&mut self, keys: &[&str]) -> Vec<String> {
		let mut args = vec!["SDIFF"];
		args.extend_from_slice(keys);
		resp_to_strings(self.execute(&args))
	}

	// -- sorted set commands --

	pub fn zadd(&mut self, key: &str, pairs: &[(&str, &str)]) -> i64 {
		let mut args = vec!["ZADD".to_string(), key.to_string()];
		for (score, member) in pairs {
			args.push(score.to_string());
			args.push(member.to_string());
		}
		let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
		self.execute(&refs)
			.as_integer()
			.expect("ZADD should return integer")
	}

	pub fn zrange(&mut self, key: &str, start: i64, stop: i64) -> Vec<String> {
		let start_s = start.to_string();
		let stop_s = stop.to_string();
		resp_to_strings(self.execute(&["ZRANGE", key, &start_s, &stop_s]))
	}

	pub fn zscore(&mut self, key: &str, member: &str) -> String {
		match self.execute(&["ZSCORE", key, member]) {
			RespValue::Null => String::new(),
			resp => resp.to_string_lossy().expect("unexpected ZSCORE response"),
		}
	}

	pub fn zrem(&mut self, key: &str, members: &[&str]) -> i64 {
		let mut args = vec!["ZREM", key];
		args.extend_from_slice(members);
		self.execute(&args)
			.as_integer()
			.expect("ZREM should return integer")
	}

	pub fn zcard(&mut self, key: &str) -> i64 {
		self.execute(&["ZCARD", key])
			.as_integer()
			.expect("ZCARD should return integer")
	}

	// -- expiry commands --

	pub fn expire(&mut self, key: &str, seconds: u64) -> bool {
		let secs = seconds.to_string();
		self.execute(&["EXPIRE", key, &secs])
			.as_integer()
			.expect("EXPIRE should return integer")
			== 1
	}

	pub fn ttl(&mut self, key: &str) -> i64 {
		self.execute(&["TTL", key])
			.as_integer()
			.expect("TTL should return integer")
	}

	// -- client commands --

	pub fn client_id(&mut self) -> i64 {
		self.execute(&["CLIENT", "ID"])
			.as_integer()
			.expect("CLIENT ID should return integer")
	}

	pub fn client_setname(&mut self, name: &str) -> String {
		self.execute(&["CLIENT", "SETNAME", name])
			.to_string_lossy()
			.expect("unexpected CLIENT SETNAME response")
	}

	pub fn client_getname(&mut self) -> String {
		match self.execute(&["CLIENT", "GETNAME"]) {
			RespValue::Null => String::new(),
			resp => resp
				.to_string_lossy()
				.expect("unexpected CLIENT GETNAME response"),
		}
	}

	pub fn client_list(&mut self) -> String {
		self.execute(&["CLIENT", "LIST"])
			.to_string_lossy()
			.expect("unexpected CLIENT LIST response")
	}
}

impl Drop for MockNimbisClient {
	fn drop(&mut self) {
		let _ = self.stream.shutdown(Shutdown::Both);
	}
}

fn configure_stream(stream: &TcpStream) -> std::io::Result<()> {
	// Bound client I/O so a broken server response cannot hang the test run.
	stream.set_read_timeout(Some(Duration::from_secs(5)))?;
	stream.set_write_timeout(Some(Duration::from_secs(5)))?;
	stream.set_nodelay(true)?;
	Ok(())
}
