use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use bytes::Bytes;
use nimbis::config::SERVER_CONF;
use nimbis::config::ServerConfig;
use nimbis::server::Server;
use resp::RespEncoder;
use resp::RespParseResult;
use resp::RespParser;
use resp::RespValue;

use super::utils::pick_free_port;

pub struct MockNimbis {
	stream: TcpStream,
	_runtime: tokio::runtime::Runtime,
}

impl MockNimbis {
	pub fn new() -> Self {
		let port = pick_free_port().expect("pick free port");
		let data_dir = tempfile::tempdir().expect("create temp dir");
		let data_path = data_dir.path().display().to_string();
		let _ = data_dir.keep();

		let config = ServerConfig {
			host: "127.0.0.1".to_string(),
			port,
			data_path,
			save: "".to_string(),
			appendonly: "no".to_string(),
			log_level: "error".to_string(),
			log_output: "terminal".to_string(),
			log_rotation: "daily".to_string(),
			worker_threads: 2,
		};

		SERVER_CONF.init(config.clone());
		SERVER_CONF.update(config);

		let runtime = tokio::runtime::Builder::new_multi_thread()
			.enable_all()
			.build()
			.expect("build tokio runtime");
		runtime.spawn(async move {
			if let Ok(server) = Server::new().await {
				let _ = server.run().await;
			}
		});

		wait_until_ready(port);

		let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to nimbis");
		Self {
			stream,
			_runtime: runtime,
		}
	}

	fn execute(&mut self, args: &[&str]) -> RespValue {
		let req = RespValue::array(
			args.iter()
				.map(|arg| RespValue::bulk_string(Bytes::copy_from_slice(arg.as_bytes()))),
		);
		self.stream
			.write_all(&req.encode().expect("encode request"))
			.expect("write request");
		read_one_resp(&mut self.stream)
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

	#[allow(dead_code)]
	pub fn flushdb(&mut self) -> bool {
		matches!(
			self.execute(&["FLUSHDB"]),
			RespValue::SimpleString(s) if s.as_ref() == b"OK"
		)
	}

	// -- string commands --

	pub fn del(&mut self, key: &str) -> i64 {
		self.execute(&["DEL", key])
			.as_integer()
			.expect("DEL should return integer")
	}

	pub fn exists(&mut self, key: &str) -> bool {
		self.execute(&["EXISTS", key])
			.as_integer()
			.expect("EXISTS should return integer")
			== 1
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
}

fn resp_to_strings(resp: RespValue) -> Vec<String> {
	match resp {
		RespValue::Array(arr) => arr
			.into_iter()
			.map(|v| match v {
				RespValue::Null => String::new(),
				other => other.to_string_lossy().unwrap_or_default(),
			})
			.collect(),
		RespValue::Null => vec![],
		_ => panic!("expected array response, got: {:?}", resp),
	}
}

fn wait_until_ready(port: u16) {
	for _ in 0..30 {
		if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
			let ping = RespValue::array([RespValue::bulk_string("PING")]);
			if stream.write_all(&ping.encode().unwrap()).is_ok()
				&& let Ok(resp) = try_read_one_resp(&mut stream)
				&& matches!(resp, RespValue::SimpleString(ref s) if s == &Bytes::from_static(b"PONG"))
			{
				return;
			}
		}

		std::thread::sleep(Duration::from_millis(100));
	}

	panic!("nimbis did not become ready in time");
}

fn try_read_one_resp(stream: &mut TcpStream) -> Result<RespValue, String> {
	let mut parser = RespParser::new();
	let mut buf = Vec::with_capacity(4096);
	let mut read_chunk = [0u8; 1024];

	loop {
		let n = stream.read(&mut read_chunk).map_err(|e| e.to_string())?;
		if n == 0 {
			return Err("connection closed before full response".into());
		}

		buf.extend_from_slice(&read_chunk[..n]);
		let mut bytes = bytes::BytesMut::from(buf.as_slice());
		match parser.parse(&mut bytes) {
			RespParseResult::Complete(v) => return Ok(v),
			RespParseResult::Incomplete => {}
			RespParseResult::Error(e) => return Err(format!("RESP parse error: {}", e)),
		}
	}
}

fn read_one_resp(stream: &mut TcpStream) -> RespValue {
	try_read_one_resp(stream).expect("read RESP response")
}
