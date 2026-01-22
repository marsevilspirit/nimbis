use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use log::debug;
use log::error;
use log::warn;
use resp::RespValue;
use storage::Storage;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::client::ClientSession;
use crate::cmd::CmdTable;

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
								tokio::spawn(async move {
									let mut session = ClientSession::new(socket, peers);
									if let Err(e) = session.run().await {
										error!("Client session error: {}", e);
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
