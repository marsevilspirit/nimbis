use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use bytes::Bytes;
use bytes::BytesMut;
use dashmap::DashMap;
use fastrace::trace;
use log::debug;
use resp::RespEncoder;
use resp::RespParseResult;
use resp::RespParser;
use resp::RespValue;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::cmd::CmdContext;
use crate::cmd::ParsedCmd;
use crate::dispatcher::CommandDispatcher;
use crate::worker::WorkerMessage;

static NEXT_CLIENT_SESSION_ID: AtomicI64 = AtomicI64::new(1);

pub fn next_client_session_id() -> i64 {
	NEXT_CLIENT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, Default)]
pub struct ClientSession {
	pub id: i64,
	pub name: Option<Bytes>,
}

#[derive(Debug, Clone, Default)]
pub struct ClientSessions {
	sessions: Arc<DashMap<i64, ClientSession>>,
}

impl ClientSessions {
	pub fn new() -> Self {
		Self {
			sessions: Arc::new(DashMap::new()),
		}
	}

	pub fn register(&self, client_id: i64) {
		self.sessions
			.entry(client_id)
			.or_insert_with(|| ClientSession {
				id: client_id,
				name: None,
			});
	}

	pub fn unregister(&self, client_id: i64) {
		self.sessions.remove(&client_id);
	}

	pub fn set_name(&self, client_id: i64, name: Bytes) -> bool {
		if let Some(mut session) = self.sessions.get_mut(&client_id) {
			session.name = Some(name);
			return true;
		}

		false
	}

	pub fn get_name(&self, client_id: i64) -> Option<Bytes> {
		self.sessions
			.get(&client_id)
			.and_then(|session| session.name.clone())
	}

	pub fn list(&self) -> Vec<(i64, Option<Bytes>)> {
		let mut entries = self
			.sessions
			.iter()
			.map(|entry| (*entry.key(), entry.value().name.clone()))
			.collect::<Vec<_>>();

		entries.sort_by_key(|(client_id, _)| *client_id);
		entries
	}
}

pub struct ClientConnection {
	socket: TcpStream,
	dispatcher: CommandDispatcher,
	parser: RespParser,
}

impl ClientConnection {
	pub fn new(
		socket: TcpStream,
		peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
		ctx: CmdContext,
	) -> Self {
		Self {
			socket,
			dispatcher: CommandDispatcher::new(peers, ctx),
			parser: RespParser::new(),
		}
	}

	#[trace]
	pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let mut buffer = BytesMut::with_capacity(4096);
		debug!("Client connection started");

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
