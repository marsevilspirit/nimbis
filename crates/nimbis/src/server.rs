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
use tracing::debug;
use tracing::error;
use tracing::info;

use crate::cmd::CmdTable;
use crate::cmd::ParsedCmd;
use crate::config::SERVER_CONF;

pub struct Server {
	storage: Arc<Storage>,
	cmd_table: Arc<CmdTable>,
}

impl Server {
	// Create a new server instance
	pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
		// Ensure data directory exists
		let data_path = &SERVER_CONF.load().data_path;
		std::fs::create_dir_all(data_path)?;
		let storage = Arc::new(Storage::open(data_path).await?);
		let cmd_table = Arc::new(CmdTable::new());

		Ok(Self { storage, cmd_table })
	}

	pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let addr = &SERVER_CONF.load().addr;
		let listener = TcpListener::bind(addr).await?;
		info!("Nimbis server listening on {}", addr);

		loop {
			match listener.accept().await {
				Ok((socket, addr)) => {
					debug!("New client connected from {}", addr);

					// Spawn a task to handle this client
					let storage = self.storage.clone();
					let cmd_table = self.cmd_table.clone();
					tokio::spawn(async move {
						if let Err(e) = handle_client(socket, storage, cmd_table).await {
							error!("Error handling client: {}", e);
						}
					});
				}
				Err(e) => {
					error!("Error accepting connection: {}", e);
					tokio::time::sleep(std::time::Duration::from_millis(500)).await;
				}
			}
		}
	}
}

async fn handle_client(
	mut socket: TcpStream,
	storage: Arc<Storage>,
	cmd_table: Arc<CmdTable>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let mut parser = RespParser::new();
	let mut buffer = BytesMut::with_capacity(4096);

	loop {
		let n = match socket.read_buf(&mut buffer).await {
			Ok(n) => n,
			Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
				debug!("Connection reset by peer");
				return Ok(());
			}
			Err(e) => return Err(e.into()),
		};

		if n == 0 {
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

					// Look up the command
					let Some(cmd) = cmd_table.get_cmd(&parsed_cmd.name) else {
						let error_response =
							RespValue::error(format!("ERR unknown command '{}'", parsed_cmd.name));
						socket.write_all(&error_response.encode()?).await?;
						continue;
					};

					// Acquire lock for the key (if any)
					// For now, we lock the first argument if it exists
					let response = if let Some(key) = parsed_cmd.args.first() {
						let _guard = storage.lock_manager.lock(key).await;
						cmd.execute(&storage, &parsed_cmd.args).await
					} else {
						// No key to lock (e.g., PING, FLUSHDB)
						cmd.execute(&storage, &parsed_cmd.args).await
					};

					socket.write_all(&response.encode()?).await?;
				}
				RespParseResult::Incomplete => {
					break;
				}
				RespParseResult::Error(e) => {
					let error_response = RespValue::error(format!("ERR Protocol error: {}", e));
					match socket.write_all(&error_response.encode()?).await {
						Err(e) if e.kind() != std::io::ErrorKind::ConnectionReset => {
							return Err(e.into());
						}
						_ => {}
					}
					return Err(e.into());
				}
			}
		}
	}
}
