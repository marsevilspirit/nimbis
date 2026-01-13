use std::collections::HashMap;
use std::hash::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use bytes::BytesMut;
use resp::RespEncoder;
use resp::RespParseResult;
use resp::RespParser;
use resp::RespValue;
use storage::Storage;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::debug;
use tracing::error;
use tracing::warn;

use crate::cmd::CmdTable;
use crate::cmd::ParsedCmd;

pub struct CmdRequest {
	pub(crate) cmd_name: String,
	pub(crate) args: Vec<Bytes>,
	pub(crate) resp_tx: oneshot::Sender<RespValue>,
}

pub enum WorkerMessage {
	NewConnection(TcpStream),
	CmdRequest(CmdRequest),
}

pub struct Worker {
	pub(crate) tx: mpsc::UnboundedSender<WorkerMessage>,
	// Keep handle to join on shutdown if needed
	_thread_handle: thread::JoinHandle<()>,
}

impl Worker {
	pub fn new(
		tx: mpsc::UnboundedSender<WorkerMessage>,
		mut rx: mpsc::UnboundedReceiver<WorkerMessage>,
		peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
		storage: Arc<Storage>,
		cmd_table: Arc<CmdTable>,
	) -> Self {
		let thread_handle = thread::spawn(move || {
			let rt = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.unwrap();

			rt.block_on(async move {
				while let Some(msg) = rx.recv().await {
					match msg {
						WorkerMessage::NewConnection(socket) => {
							let peers = peers.clone();
							tokio::spawn(async move {
								if let Err(e) = handle_client(socket, peers).await {
									error!("Error handling client: {}", e);
								}
							});
						}
						WorkerMessage::CmdRequest(req) => {
							let response = match cmd_table.get_cmd(&req.cmd_name) {
								Some(cmd) => cmd.execute(&storage, &req.args).await,
								None => RespValue::error(format!(
									"ERR unknown command '{}'",
									req.cmd_name.to_lowercase()
								)),
							};
							if let Err(resp) = req.resp_tx.send(response) {
								warn!(
									"Failed to send response for command '{}'; receiver dropped. Dropped response: {:?}",
									req.cmd_name, resp
								);
							}
						}
					}
				}
			});
		});

		Self {
			tx,
			_thread_handle: thread_handle,
		}
	}
}

async fn handle_client(
	socket: TcpStream,
	peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let (mut rd, mut wr) = socket.into_split();
	let (w_tx, mut w_rx) = mpsc::channel::<oneshot::Receiver<RespValue>>(1000);

	// Spawn writer task
	tokio::spawn(async move {
		while let Some(resp_rx) = w_rx.recv().await {
			match resp_rx.await {
				Ok(response) => match response.encode() {
					Ok(encoded) => {
						if let Err(e) = wr.write_all(&encoded).await {
							debug!("Write error: {}", e);
							break;
						}
					}
					Err(e) => {
						error!("Failed to encode response: {}", e);
						break;
					}
				},
				Err(_) => {
					warn!("Response sender dropped");
					break;
				}
			}
		}
	});

	let mut parser = RespParser::new();
	let mut buffer = BytesMut::with_capacity(4096);

	loop {
		let n = match rd.read_buf(&mut buffer).await {
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

					// Calculate target worker using hash of the first key
					let hash_key = parsed_cmd.args.first().cloned().unwrap_or_default();
					let mut hasher = DefaultHasher::new();
					hash_key.hash(&mut hasher);
					// peers contains ALL workers (including self), so len() is the total worker count.
					let target_worker_idx = (hasher.finish() as usize) % peers.len();

					let (resp_tx, resp_rx) = oneshot::channel();

					// Always route through channel to ensure serial execution
					if let Some(sender) = peers.get(&target_worker_idx) {
						sender.send(WorkerMessage::CmdRequest(CmdRequest {
							cmd_name: parsed_cmd.name,
							args: parsed_cmd.args,
							resp_tx,
						}))?;
					} else {
						// Should not happen if topology is correct
						error!("Worker {} not found", target_worker_idx);
						return Err("Internal error: worker not found".into());
					}

					if w_tx.send(resp_rx).await.is_err() {
						return Err("Writer task closed".into());
					}
				}
				RespParseResult::Incomplete => {
					break;
				}
				RespParseResult::Error(e) => {
					let (tx, rx) = oneshot::channel();
					let _ = tx.send(RespValue::error(format!("ERR Protocol error: {}", e)));
					let _ = w_tx.send(rx).await;
					return Err(e.into());
				}
			}
		}
	}
}
