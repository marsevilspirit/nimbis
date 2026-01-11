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
use tokio::task::LocalSet;
use tracing::debug;
use tracing::error;

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
		id: usize,
		tx: mpsc::UnboundedSender<WorkerMessage>,
		mut rx: mpsc::UnboundedReceiver<WorkerMessage>,
		peers: HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>,
		storage: Arc<Storage>,
		cmd_table: Arc<CmdTable>,
	) -> Self {
		let thread_handle = thread::spawn(move || {
			let rt = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.unwrap();

			let local = LocalSet::new();

			local.block_on(&rt, async move {
				while let Some(msg) = rx.recv().await {
					match msg {
						WorkerMessage::NewConnection(socket) => {
							let storage = storage.clone();
							let cmd_table = cmd_table.clone();
							let peers = peers.clone();
							tokio::task::spawn_local(async move {
								if let Err(e) =
									handle_client(socket, id, storage, cmd_table, peers).await
								{
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
							let _ = req.resp_tx.send(response);
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
	mut socket: TcpStream,
	worker_id: usize,
	storage: Arc<Storage>,
	cmd_table: Arc<CmdTable>,
	peers: HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>,
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

					// Calculate target worker using hash of the first key
					let hash_key = parsed_cmd.args.first().cloned().unwrap_or_default();
					let mut hasher = DefaultHasher::new();
					hash_key.hash(&mut hasher);
					let target_worker_idx = (hasher.finish() as usize) % peers.len(); // peers.len() is correct because it includes all workers
					// Wait, logic for peers.len() needs to be correct.
					// If we have N workers, ids are 0..N-1.
					// We need to know total workers.
					// Let's assume peers contains ALL workers including self?
					// Or we pass total count.
					// Simple fix: peers map allows looking up sender.
					// But for modulo we need total count.
					// Let's assume peers map logic is: peers contains everyone else.
					// So total = peers.len() + 1.
					// Indices should be consistent.

					// If peers contains only *other* workers, then len() + 1.
					// In Server::new logic (to be written), it's easier to give everyone a map of ALL workers (including self) to make looking up by index easy.
					// But sender to self? We have `WorkerMessage::CmdRequest`.
					// If we send to self via channel, we are async.
					// If we execute directly, we are sync (in terms of event loop).
					// Dragonfly model usually executes local commands immediately.
					// But generic `execute` is async.
					// If we send to self channel, it puts it at end of queue.
					// For now, let's just use channels for everyone to keep it uniform?
					// No, local execution is preferred for latency.
					// Let's assume peers contains ALL workers.

					let (resp_tx, resp_rx) = oneshot::channel();

					if target_worker_idx == worker_id {
						// Local execution
						// We can't await execute here directly if we want to run it on the same fiber blocking?
						// `execute` is async. If we await it here, we yield back to runtime.
						// Since we are in `spawn_local`, we are a fiber.
						// Other fibers can run.
						// This seems fine.
						let response = match cmd_table.get_cmd(&parsed_cmd.name) {
							Some(cmd) => cmd.execute(&storage, &parsed_cmd.args).await,
							None => RespValue::error(format!(
								"ERR unknown command '{}'",
								parsed_cmd.name.to_lowercase()
							)),
						};
						if let Err(e) = socket.write_all(&response.encode()?).await {
							if e.kind() == std::io::ErrorKind::ConnectionReset {
								debug!("Connection reset by peer");
								return Ok(());
							}
							return Err(e.into());
						}
					} else {
						// Remote execution
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

						let response = resp_rx.await.map_err(|_| "worker dropped request")?;
						if let Err(e) = socket.write_all(&response.encode()?).await {
							if e.kind() == std::io::ErrorKind::ConnectionReset {
								debug!("Connection reset by peer");
								return Ok(());
							}
							return Err(e.into());
						}
					}
				}
				RespParseResult::Incomplete => {
					break;
				}
				RespParseResult::Error(e) => {
					let error_response = RespValue::error(format!("ERR Protocol error: {}", e));
					if let Err(write_err) = socket.write_all(&error_response.encode()?).await {
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
