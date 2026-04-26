use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use fastrace::future::FutureExt;
use fastrace::prelude::Span;
use fastrace::prelude::SpanContext;
use fastrace::trace;
use log::debug;
use log::warn;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::GCTX;
use crate::client::ClientConnection;
use crate::client::next_client_session_id;
use crate::cmd::CmdContext;
use crate::cmd::CmdTable;

pub struct CmdRequest {
	pub(crate) cmd_name: String,
	pub(crate) args: Vec<Bytes>,
	pub(crate) ctx: CmdContext,
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
									let client_id = next_client_session_id();
									let ctx = CmdContext { client_id };
									let mut session = ClientConnection::new(socket, peers, ctx);
									GCTX!(client_sessions).register(client_id);
									if let Err(e) = session.run().await {
										debug!("Client session error: {}", e);
									}
									GCTX!(client_sessions).unregister(client_id);
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
		// Apply a 0.01% sampling rate for tracing to prevent massive gRPC payload and
		// reduce overhead.
		let is_sampled = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap_or_default()
			.subsec_nanos()
			.is_multiple_of(10000);
		let span_context = SpanContext::random().sampled(is_sampled);
		let root_span = Span::root(fastrace::func_path!(), span_context).with_properties(|| {
			[
				("cmd", req.cmd_name.clone()),
				("client_id", req.ctx.client_id.to_string()),
			]
		});

		Self::handle_cmd_request_inner(req, cmd_table, storage)
			.in_span(root_span)
			.await;
	}

	#[trace]
	async fn handle_cmd_request_inner(req: CmdRequest, cmd_table: &CmdTable, storage: &Storage) {
		let response = match cmd_table.get_cmd(&req.cmd_name) {
			Some(cmd) => cmd.execute(storage, &req.args, &req.ctx).await,
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
