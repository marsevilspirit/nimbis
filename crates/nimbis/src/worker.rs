use std::collections::HashMap;
use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use bytes::BytesMut;
use kimojio::ReceiverUnbounded;
use kimojio::Runtime;
use kimojio::SenderUnbounded;
use kimojio::configuration::Configuration;
use resp::RespEncoder;
use resp::RespParseResult;
use resp::RespParser;
use resp::RespValue;
use storage::Storage;
use tracing::debug;
use tracing::error;
use tracing::warn;

use crate::cmd::CmdTable;
use crate::cmd::ParsedCmd;

pub struct CmdRequest {
	pub(crate) cmd_name: String,
	pub(crate) args: Vec<Bytes>,
	pub(crate) resp_tx: SenderUnbounded<RespValue>,
}

pub enum WorkerMessage {
	NewConnection(TcpStreamWrapper),
	CmdRequest(CmdRequest),
}

// Wrapper for TCP stream to work with Kimojio
pub struct TcpStreamWrapper {
	// We'll use Kimojio's OwnedFdStream for actual I/O
	// For now, we need to convert tokio::net::TcpStream to a raw fd
	pub fd: std::os::fd::RawFd,
}

impl TcpStreamWrapper {
	pub fn from_tokio_stream(stream: tokio::net::TcpStream) -> std::io::Result<Self> {
		use std::os::fd::AsRawFd;
		let fd = stream.as_raw_fd();
		// Important: we need to prevent tokio from closing the fd
		// This is a temporary solution - ideally we'd use Kimojio for the entire stack
		std::mem::forget(stream);
		Ok(Self { fd })
	}
}

pub struct Worker {
	pub(crate) tx: SenderUnbounded<WorkerMessage>,
	// Keep handle to join on shutdown if needed
	_thread_handle: thread::JoinHandle<()>,
}

impl Worker {
	pub fn new(
		tx: SenderUnbounded<WorkerMessage>,
		mut rx: ReceiverUnbounded<WorkerMessage>,
		peers: Arc<HashMap<usize, SenderUnbounded<WorkerMessage>>>,
		storage: Arc<Storage>,
		cmd_table: Arc<CmdTable>,
		worker_idx: usize,
	) -> Self {
		let thread_handle = thread::spawn(move || {
			let configuration = Configuration::default();
			let mut rt = Runtime::new(worker_idx as u8, configuration);

			let result = rt.block_on(async move {
				// Track active connections with their state
				let mut connections: HashMap<usize, ConnectionState> = HashMap::new();
				let mut next_conn_id = 0usize;

				loop {
					// Check for new messages from the channel
					match rx.try_recv() {
						Ok(msg) => {
							match msg {
								WorkerMessage::NewConnection(stream_wrapper) => {
									let conn_id = next_conn_id;
									next_conn_id += 1;

									// Create connection state
									let conn_state = ConnectionState::new(
										conn_id,
										stream_wrapper,
										peers.clone(),
									);

									connections.insert(conn_id, conn_state);
									debug!("New connection {} registered", conn_id);
								}
								WorkerMessage::CmdRequest(req) => {
									let response = match cmd_table.get_cmd(&req.cmd_name) {
										Some(cmd) => cmd.execute(&storage, &req.args).await,
										None => RespValue::error(format!(
											"ERR unknown command '{}'",
											req.cmd_name.to_lowercase()
										)),
									};
									if let Err(resp) = req.resp_tx.send(response).await {
										warn!(
											"Failed to send response for command '{}'; receiver dropped. Dropped response: {:?}",
											req.cmd_name, resp
										);
									}
								}
							}
						}
						Err(kimojio::ChannelError::Empty) => {
							// No messages, process existing connections
						}
						Err(kimojio::ChannelError::Closed) => {
							debug!("Channel closed, worker shutting down");
							break;
						}
					}

					// Process all active connections
					let mut to_remove = Vec::new();
					for (conn_id, conn_state) in connections.iter_mut() {
						match conn_state.process_once(&storage, &cmd_table).await {
							Ok(ConnectionStatus::KeepAlive) => {
								// Connection is still active
							}
							Ok(ConnectionStatus::Closed) => {
								debug!("Connection {} closed gracefully", conn_id);
								to_remove.push(*conn_id);
							}
							Err(e) => {
								error!("Error processing connection {}: {}", conn_id, e);
								to_remove.push(*conn_id);
							}
						}
					}

					// Remove closed connections
					for conn_id in to_remove {
						connections.remove(&conn_id);
					}

					// Small yield to prevent busy waiting
					// In a real implementation, we'd use Kimojio's event system
					kimojio::timer::sleep(std::time::Duration::from_micros(100)).await;
				}

				Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
			});

			if let Some(Err(e)) = result {
				error!("Worker runtime panicked: {:?}", e);
			}
		});

		Self {
			tx,
			_thread_handle: thread_handle,
		}
	}
}

enum ConnectionStatus {
	KeepAlive,
	Closed,
}

struct ConnectionState {
	conn_id: usize,
	fd: kimojio::OwnedFd,
	parser: RespParser,
	buffer: BytesMut,
	peers: Arc<HashMap<usize, SenderUnbounded<WorkerMessage>>>,
}

impl ConnectionState {
	fn new(
		conn_id: usize,
		stream_wrapper: TcpStreamWrapper,
		peers: Arc<HashMap<usize, SenderUnbounded<WorkerMessage>>>,
	) -> Self {
		// SAFETY: We own the fd and it's valid
		let fd = unsafe { kimojio::OwnedFd::new(stream_wrapper.fd) };

		Self {
			conn_id,
			fd,
			parser: RespParser::new(),
			buffer: BytesMut::with_capacity(4096),
			peers,
		}
	}

	async fn process_once(
		&mut self,
		storage: &Arc<Storage>,
		cmd_table: &Arc<CmdTable>,
	) -> Result<ConnectionStatus, Box<dyn std::error::Error + Send + Sync>> {
		use kimojio::AsyncStreamRead;
		use kimojio::AsyncStreamWrite;

		// Try to read data
		let mut temp_buf = vec![0u8; 4096];
		match kimojio::operations::read(&self.fd, &mut temp_buf).await {
			Ok(0) => {
				// Connection closed
				return Ok(ConnectionStatus::Closed);
			}
			Ok(n) => {
				self.buffer.extend_from_slice(&temp_buf[..n]);
			}
			Err(e) if e.code() == kimojio::EAGAIN => {
				// No data available, return and try again later
				return Ok(ConnectionStatus::KeepAlive);
			}
			Err(e) => {
				return Err(format!("Read error: {:?}", e).into());
			}
		}

		// Parse RESP data
		loop {
			match self.parser.parse(&mut self.buffer) {
				RespParseResult::Complete(value) => {
					let parsed_cmd: ParsedCmd = value.try_into()?;

					// Calculate target worker using hash of the first key
					let hash_key = parsed_cmd.args.first().cloned().unwrap_or_default();
					let mut hasher = DefaultHasher::new();
					hash_key.hash(&mut hasher);
					let target_worker_idx = (hasher.finish() as usize) % self.peers.len();

					let (resp_tx, mut resp_rx) = kimojio::async_channel_unbounded();

					// Route through channel to ensure serial execution
					if let Some(sender) = self.peers.get(&target_worker_idx) {
						sender
							.send(WorkerMessage::CmdRequest(CmdRequest {
								cmd_name: parsed_cmd.name,
								args: parsed_cmd.args,
								resp_tx,
							}))
							.await?;
					} else {
						error!("Worker {} not found", target_worker_idx);
						return Err("Internal error: worker not found".into());
					}

					// Wait for response
					let response = resp_rx.recv().await.map_err(|_| "worker dropped request")?;

					// Write response
					let encoded = response.encode()?;
					kimojio::operations::write(&self.fd, &encoded).await?;
				}
				RespParseResult::Incomplete => {
					break;
				}
				RespParseResult::Error(e) => {
					let error_response = RespValue::error(format!("ERR Protocol error: {}", e));
					let encoded = error_response.encode()?;
					kimojio::operations::write(&self.fd, &encoded).await?;
					return Err(e.into());
				}
			}
		}

		Ok(ConnectionStatus::KeepAlive)
	}
}
