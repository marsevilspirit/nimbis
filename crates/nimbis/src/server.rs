use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;

use bytes::BytesMut;
use resp::RespEncoder;
use resp::RespParseResult;
use resp::RespParser;
use resp::RespValue;
use storage::Storage;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tracing::debug;
use tracing::error;
use tracing::info;

use crate::cmd::CmdTable;
use crate::cmd::ParsedCmd;
use crate::config::SERVER_CONF;
use crate::worker::CmdRequest;
use crate::worker::Worker;

pub struct Server {
	workers: Vec<Worker>,
}

impl Server {
	// Create a new server instance
	pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
		// Ensure data directory exists
		let data_path = &SERVER_CONF.load().data_path;
		std::fs::create_dir_all(data_path)?;
		let storage = Arc::new(Storage::open(data_path).await?);
		let cmd_table = Arc::new(CmdTable::new());

		let workers_num = num_cpus::get();
		let mut workers = Vec::with_capacity(workers_num);
		for _ in 0..workers_num {
			workers.push(Worker::new(storage.clone(), cmd_table.clone()));
		}

		Ok(Self { workers })
	}

	pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let addr = &SERVER_CONF.load().addr;
		let listener = TcpListener::bind(addr).await?;
		let workers = Arc::new(self.workers);
		info!("Nimbis server listening on {}", addr);

		loop {
			match listener.accept().await {
				Ok((socket, addr)) => {
					debug!("New client connected from {}", addr);
					let clone_workers = workers.clone();

					tokio::spawn(async move {
						if let Err(e) = handle_client(socket, clone_workers).await {
							error!("Error handling client: {}", e);
						}
					});
				}
				Err(e) => {
					error!("Error accepting connection: {}", e);
				}
			}
		}
	}
}

async fn handle_client(
	mut socket: TcpStream,
	workers: Arc<Vec<Worker>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let mut parser = RespParser::new();
	let mut buffer = BytesMut::with_capacity(4096);

	loop {
		let n = match socket.read_buf(&mut buffer).await {
			Ok(n) => n,
			Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
				// Connection reset by peer (e.g. client crashed or closed abruptly)
				// Treat as normal disconnect
				debug!("Connection reset by peer");
				return Ok(());
			}
			Err(e) => return Err(e.into()),
		};

		if n == 0 {
			// Connection closed
			if buffer.is_empty() {
				return Ok(());
			} else {
				return Err("Connection closed with incomplete data".into());
			}
		}

		loop {
			match parser.parse(&mut buffer) {
				RespParseResult::Complete(value) => {
					let parsed_cmd: ParsedCmd = value.try_into()?;

					let hash_key = parsed_cmd.args.first().cloned().unwrap_or_default();

					let mut hasher = DefaultHasher::new();
					hash_key.hash(&mut hasher);
					let worker_idx = (hasher.finish() as usize) % workers.len();

					let (resp_tx, resp_rx) = oneshot::channel();

					workers[worker_idx].tx.send(CmdRequest {
						cmd_name: parsed_cmd.name,
						args: parsed_cmd.args,
						resp_tx,
					})?;

					let response = resp_rx.await.map_err(|_| "worker dropped request")?;

					let encoded = response.encode()?;
					if let Err(e) = socket.write_all(&encoded).await {
						if e.kind() == std::io::ErrorKind::ConnectionReset {
							debug!("Connection reset by peer");
							return Ok(());
						}
						return Err(e.into());
					}
				}
				RespParseResult::Incomplete => {
					break;
				}
				RespParseResult::Error(e) => {
					let error_response = RespValue::error(format!("ERR Protocol error: {}", e));
					let encoded = error_response.encode()?;
					if let Err(write_err) = socket.write_all(&encoded).await {
						// If we can't write the error response because connection is reset, just give up
						if write_err.kind() != std::io::ErrorKind::ConnectionReset {
							return Err(write_err.into());
						}
					}
					return Err(e.into());
				}
			}
		}
	}
}
