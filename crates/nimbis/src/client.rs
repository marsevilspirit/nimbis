use std::collections::HashMap;
use std::sync::Arc;

use bytes::BytesMut;
use log::debug;
use resp::RespEncoder;
use resp::RespParseResult;
use resp::RespParser;
use resp::RespValue;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::cmd::ParsedCmd;
use crate::dispatcher::CommandDispatcher;
use crate::worker::WorkerMessage;

pub struct ClientSession {
	socket: TcpStream,
	dispatcher: CommandDispatcher,
	parser: RespParser,
}

impl ClientSession {
	pub fn new(
		socket: TcpStream,
		peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	) -> Self {
		Self {
			socket,
			dispatcher: CommandDispatcher::new(peers),
			parser: RespParser::new(),
		}
	}

	pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let mut buffer = BytesMut::with_capacity(4096);

		loop {
			let n = match self.socket.read_buf(&mut buffer).await {
				Ok(n) => n,
				Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
					debug!("Connection reset by peer");
					return Ok(());
				}
				Err(e) => return Err(e.into()),
			};
			debug!("Read {} bytes from socket", n);

			if n == 0 {
				if buffer.is_empty() {
					return Ok(());
				} else {
					return Err("Connection closed with incomplete data".into());
				}
			}

			self.dispatcher.reset();

			loop {
				match self.parser.parse(&mut buffer) {
					RespParseResult::Complete(value) => {
						let parsed_cmd: ParsedCmd = match value.try_into() {
							Ok(cmd) => cmd,
							Err(e) => {
								let error_response =
									RespValue::error(format!("ERR Protocol error: {}", e));
								self.socket.write_all(&error_response.encode()?).await?;
								return Err(e.into());
							}
						};
						self.dispatcher.dispatch(parsed_cmd);
					}
					RespParseResult::Incomplete => {
						break;
					}
					RespParseResult::Error(e) => {
						let error_response = RespValue::error(format!("ERR Protocol error: {}", e));
						match self.socket.write_all(&error_response.encode()?).await {
							Err(e) if e.kind() != std::io::ErrorKind::ConnectionReset => {
								return Err(e.into());
							}
							_ => {}
						}
						return Err(e.into());
					}
				}
			}

			self.dispatcher.dispatch_batches().await?;
			self.dispatcher.await_responses(&mut self.socket).await?;
		}
	}
}
