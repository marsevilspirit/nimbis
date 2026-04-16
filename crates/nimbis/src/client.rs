use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use bytes::Bytes;
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

#[derive(Clone)]
pub struct ClientSessions(Arc<RwLock<HashMap<i64, ClientSession>>>);

impl ClientSessions {
	pub fn new() -> Self {
		Self(Arc::new(RwLock::new(HashMap::new())))
	}
}

impl Default for ClientSessions {
	fn default() -> Self {
		Self::new()
	}
}

pub fn register_client(client_sessions: &ClientSessions, client_id: i64) {
	if let Ok(mut guard) = client_sessions.0.write() {
		guard.entry(client_id).or_insert_with(|| ClientSession {
			id: client_id,
			name: None,
		});
	}
}

pub fn unregister_client(client_sessions: &ClientSessions, client_id: i64) {
	if let Ok(mut guard) = client_sessions.0.write() {
		guard.remove(&client_id);
	}
}

pub fn set_client_name(client_sessions: &ClientSessions, client_id: i64, name: Bytes) -> bool {
	if let Ok(mut guard) = client_sessions.0.write()
		&& let Some(session) = guard.get_mut(&client_id)
	{
		session.name = Some(name);
		return true;
	}

	false
}

pub fn get_client_name(client_sessions: &ClientSessions, client_id: i64) -> Option<Bytes> {
	client_sessions.0.read().ok().and_then(|guard| {
		guard
			.get(&client_id)
			.and_then(|session| session.name.clone())
	})
}

pub fn list_clients(client_sessions: &ClientSessions) -> Vec<(i64, Option<Bytes>)> {
	let mut entries = client_sessions
		.0
		.read()
		.ok()
		.map(|guard| {
			guard
				.iter()
				.map(|(client_id, session)| (*client_id, session.name.clone()))
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();

	entries.sort_by_key(|(client_id, _)| *client_id);
	entries
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
