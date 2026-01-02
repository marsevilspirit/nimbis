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

		// Open database
		let storage = Storage::open(data_path).await?;
		Ok(Self {
			storage: Arc::new(storage),
			cmd_table: Arc::new(CmdTable::new()),
		})
	}

	pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let addr = &SERVER_CONF.load().addr;
		let listener = TcpListener::bind(addr).await?;
		info!("Nimbis server listening on {}", addr);

		loop {
			match listener.accept().await {
				Ok((socket, addr)) => {
					debug!("New client connected from {}", addr);
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

					// Log the command being executed
					tracing::debug!(
						command = %parsed_cmd.name,
						args = ?parsed_cmd.args,
						"Executing command"
					);

					let response = match cmd_table.get_cmd(&parsed_cmd.name) {
						Some(cmd) => cmd.execute(&storage, &parsed_cmd.args).await,
						None => RespValue::error(format!(
							"ERR unknown command '{}'",
							parsed_cmd.name.to_lowercase()
						)),
					};

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
