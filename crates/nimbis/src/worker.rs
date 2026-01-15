use std::collections::HashMap;
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
	CmdBatch(Vec<CmdRequest>),
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
				debug!("Worker thread {} started", id);
				// Use a small buffer to aggregate messages from the channel
				let mut batch_buffer = Vec::with_capacity(64);

				while let Some(msg) = rx.recv().await {
					debug!("Worker {} received message", id);
					batch_buffer.push(msg);

					// Try to drain more messages from the channel if available (Smart Batching)
					// Limit to 256 to avoid starvation or long delays
					while batch_buffer.len() < 256 {
						match rx.try_recv() {
							Ok(msg) => batch_buffer.push(msg),
							Err(_) => break, // Empty or Closed
						}
					}

					for msg in batch_buffer.drain(..) {
						match msg {
							WorkerMessage::NewConnection(socket) => {
								let peers = peers.clone();
								let storage = storage.clone();
								let cmd_table = cmd_table.clone();
								tokio::spawn(async move {
									if let Err(e) =
										handle_client(socket, peers, storage, cmd_table, id).await
									{
										error!("Error handling client: {}", e);
									}
								});
							}
							WorkerMessage::CmdBatch(reqs) => {
								for req in reqs {
									Self::handle_cmd_request(req, &cmd_table, &storage).await;
								}
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

	async fn handle_cmd_request(req: CmdRequest, cmd_table: &CmdTable, storage: &Storage) {
		let response = match cmd_table.get_cmd(&req.cmd_name) {
			Some(cmd) => cmd.execute(storage, &req.args).await,
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

enum PendingResponse {
	Future(oneshot::Receiver<RespValue>),
}

async fn handle_client(
	mut socket: TcpStream,
	peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	_storage: Arc<Storage>,
	_cmd_table: Arc<CmdTable>,
	_start_worker_idx: usize,
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
		debug!("Read {} bytes from socket", n);

		if n == 0 {
			if buffer.is_empty() {
				return Ok(());
			} else {
				return Err("Connection closed with incomplete data".into());
			}
		}

		// Batching variables
		// Map from worker_idx to a list of commands for that worker
		let mut batches: HashMap<usize, Vec<CmdRequest>> = HashMap::new();
		// List of responses (either ready or future) in the order of commands received
		let mut ordered_responses: Vec<PendingResponse> = Vec::new();

		loop {
			match parser.parse(&mut buffer) {
				RespParseResult::Complete(value) => {
					let parsed_cmd: ParsedCmd = match value.try_into() {
						Ok(cmd) => cmd,
						Err(e) => {
							// Protocol error during conversion
							let error_response =
								RespValue::error(format!("ERR Protocol error: {}", e));
							socket.write_all(&error_response.encode()?).await?;
							return Err(e.into());
						}
					};

					if parsed_cmd.name.to_lowercase() == "flushdb" {
						// Broadcast FLUSHDB to all workers
						let (final_tx, final_rx) = oneshot::channel();
						ordered_responses.push(PendingResponse::Future(final_rx));

						let mut flush_rxs = Vec::with_capacity(peers.len());
						for worker_idx in 0..peers.len() {
							let (tx, rx) = oneshot::channel();
							flush_rxs.push(rx);
							let req = CmdRequest {
								cmd_name: "FLUSHDB".to_string(),
								args: parsed_cmd.args.clone(),
								resp_tx: tx,
							};
							batches.entry(worker_idx).or_default().push(req);
						}

						// Spawn a task to aggregate responses
						tokio::spawn(async move {
							let mut success = true;
							for rx in flush_rxs {
								match rx.await {
									Ok(RespValue::SimpleString(s)) if s == "OK" => {}
									_ => success = false,
								}
							}
							let result = if success {
								RespValue::SimpleString(Bytes::from_static(b"OK"))
							} else {
								RespValue::Error(Bytes::from("Failed to flush all shards"))
							};
							final_tx.send(result).ok();
						});
					} else if parsed_cmd.name.to_lowercase() == "del" && parsed_cmd.args.len() > 1 {
						// Scatter-gather DEL for multiple keys
						let (final_tx, final_rx) = oneshot::channel();
						ordered_responses.push(PendingResponse::Future(final_rx));

						let mut del_rxs = Vec::new();

						// Group keys by worker
						let mut keys_by_worker: HashMap<usize, Vec<Bytes>> = HashMap::new();
						for key in parsed_cmd.args {
							let mut hasher: u64 = 0xcbf29ce484222325;
							for byte in &key {
								hasher ^= *byte as u64;
								hasher = hasher.wrapping_mul(0x100000001b3);
							}
							let worker_idx = (hasher as usize) % peers.len();
							keys_by_worker.entry(worker_idx).or_default().push(key);
						}

						for (worker_idx, keys) in keys_by_worker {
							let (tx, rx) = oneshot::channel();
							del_rxs.push(rx);
							let req = CmdRequest {
								cmd_name: "DEL".to_string(),
								args: keys,
								resp_tx: tx,
							};
							batches.entry(worker_idx).or_default().push(req);
						}

						tokio::spawn(async move {
							let mut total_deleted = 0;
							let mut error = None;
							for rx in del_rxs {
								match rx.await {
									Ok(RespValue::Integer(n)) => total_deleted += n,
									Ok(RespValue::Error(e)) => error = Some(e),
									Ok(_) => {} // Unexpected type
									Err(_) => error = Some(Bytes::from("Internal error")),
								}
							}

							let result = if let Some(e) = error {
								RespValue::Error(e)
							} else {
								RespValue::Integer(total_deleted)
							};
							final_tx.send(result).ok();
						});
					} else {
						// Calculate target worker using hash of the first key
						let hash_key = parsed_cmd.args.first().cloned().unwrap_or_default();

						// FNV-1a 64-bit hash
						let mut hasher: u64 = 0xcbf29ce484222325;
						for byte in &hash_key {
							hasher ^= *byte as u64;
							hasher = hasher.wrapping_mul(0x100000001b3);
						}

						let target_worker_idx = (hasher as usize) % peers.len();

						// Always route via channel (even for local) to ensure serialization
						let (resp_tx, resp_rx) = oneshot::channel();
						ordered_responses.push(PendingResponse::Future(resp_rx));

						let req = CmdRequest {
							cmd_name: parsed_cmd.name,
							args: parsed_cmd.args,
							resp_tx,
						};

						batches.entry(target_worker_idx).or_default().push(req);
					}
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

		// Dispatch batches
		for (worker_idx, batch) in batches {
			debug!(
				"Dispatching batch of {} cmds to worker {}",
				batch.len(),
				worker_idx
			);
			if let Some(sender) = peers.get(&worker_idx) {
				if let Err(e) = sender.send(WorkerMessage::CmdBatch(batch)) {
					error!("Failed to send batch to worker {}: {}", worker_idx, e);
					return Err(e.into());
				}
			} else {
				error!("Worker {} not found", worker_idx);
				return Err("Internal error: worker not found".into());
			}
		}

		// Wait for responses in order and write to socket
		for response in ordered_responses {
			debug!("Waiting for response...");
			let resp_value = match response {
				PendingResponse::Future(rx) => rx.await.map_err(|_| "worker dropped request")?,
			};
			debug!("Got response, writing to socket");
			if let Err(e) = socket.write_all(&resp_value.encode()?).await {
				if e.kind() == std::io::ErrorKind::ConnectionReset {
					debug!("Connection reset by peer");
					return Ok(());
				}
				return Err(e.into());
			}
		}
	}
}
