use std::error::Error;
use std::net::TcpListener;
use std::sync::OnceLock;
use std::time::Duration;

use bytes::Bytes;
use nimbis::config::SERVER_CONF;
use nimbis::config::ServerConfig;
use nimbis::server::Server;
use resp::RespEncoder;
use resp::RespParseResult;
use resp::RespParser;
use resp::RespValue;
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::OnceCell;

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

struct TestServer {
	port: u16,
	_handle: tokio::task::JoinHandle<()>,
	_data_dir: TempDir,
}

static TEST_SERVER: OnceCell<TestServer> = OnceCell::const_new();
static LOGGER_INIT: OnceLock<()> = OnceLock::new();

pub struct MockNimbis {
	stream: TcpStream,
}

impl MockNimbis {
	pub async fn new() -> TestResult<Self> {
		let test_server = TEST_SERVER
			.get_or_try_init(|| async {
				let port = pick_free_port()?;
				let data_dir = tempfile::tempdir()?;

				let config = ServerConfig {
					host: "127.0.0.1".to_string(),
					port,
					data_path: data_dir.path().display().to_string(),
					save: "".to_string(),
					appendonly: "no".to_string(),
					log_level: "error".to_string(),
					log_output: "terminal".to_string(),
					log_rotation: "daily".to_string(),
					worker_threads: 2,
				};

				let _ = LOGGER_INIT.get_or_init(|| {
					let _ = telemetry::logger::init(
						&config.log_level,
						telemetry::logger::LogOutput::Terminal(telemetry::logger::Terminal),
					);
				});

				SERVER_CONF.init(config);
				let server = Server::new().await?;
				let handle = tokio::spawn(async move {
					let _ = server.run().await;
				});

				wait_until_ready(port).await?;

				Ok::<TestServer, Box<dyn Error + Send + Sync>>(TestServer {
					port,
					_handle: handle,
					_data_dir: data_dir,
				})
			})
			.await?;

		let stream = TcpStream::connect(("127.0.0.1", test_server.port)).await?;
		let mut mock = Self { stream };
		let _ = mock.flushdb().await?;
		Ok(mock)
	}

	pub async fn execute(&mut self, args: &[&str]) -> TestResult<RespValue> {
		let req = RespValue::array(
			args.iter()
				.map(|arg| RespValue::bulk_string(Bytes::copy_from_slice(arg.as_bytes()))),
		);
		self.stream.write_all(&req.encode()?).await?;
		read_one_resp(&mut self.stream).await
	}

	pub async fn ping(&mut self) -> TestResult<RespValue> {
		self.execute(&["PING"]).await
	}

	pub async fn set(&mut self, key: &str, value: &str) -> TestResult<RespValue> {
		self.execute(&["SET", key, value]).await
	}

	pub async fn get(&mut self, key: &str) -> TestResult<RespValue> {
		self.execute(&["GET", key]).await
	}

	pub async fn flushdb(&mut self) -> TestResult<RespValue> {
		self.execute(&["FLUSHDB"]).await
	}
}

fn pick_free_port() -> TestResult<u16> {
	let listener = TcpListener::bind("127.0.0.1:0")?;
	Ok(listener.local_addr()?.port())
}

async fn wait_until_ready(port: u16) -> TestResult<()> {
	for _ in 0..30 {
		if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)).await {
			let ping = RespValue::array([RespValue::bulk_string("PING")]);
			if stream.write_all(&ping.encode()?).await.is_ok()
				&& let Ok(resp) = read_one_resp(&mut stream).await
				&& matches!(resp, RespValue::SimpleString(ref s) if s == &Bytes::from_static(b"PONG"))
			{
				return Ok(());
			}
		}

		tokio::time::sleep(Duration::from_millis(100)).await;
	}

	Err("nimbis did not become ready in time".into())
}

async fn read_one_resp(stream: &mut TcpStream) -> TestResult<RespValue> {
	let mut parser = RespParser::new();
	let mut buf = Vec::with_capacity(4096);
	let mut read_chunk = [0u8; 1024];

	loop {
		let n = stream.read(&mut read_chunk).await?;
		if n == 0 {
			return Err("connection closed before full response".into());
		}

		buf.extend_from_slice(&read_chunk[..n]);
		let mut bytes = bytes::BytesMut::from(buf.as_slice());
		match parser.parse(&mut bytes) {
			RespParseResult::Complete(v) => return Ok(v),
			RespParseResult::Incomplete => {}
			RespParseResult::Error(e) => return Err(format!("RESP parse error: {}", e).into()),
		}
	}
}
