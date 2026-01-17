use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use resp::RespEncoder;
use resp::RespValue;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::debug;
use tracing::error;

use crate::cmd::ParsedCmd;
use crate::worker::CmdRequest;
use crate::worker::WorkerMessage;

/// Simple FNV-1a 64-bit hash for key-based worker routing
#[inline]
fn hash_key(key: &[u8]) -> u64 {
	let mut hasher: u64 = 0xcbf29ce484222325;
	for &byte in key {
		hasher ^= byte as u64;
		hasher = hasher.wrapping_mul(0x100000001b3);
	}
	hasher
}

enum PendingResponse {
	Future(oneshot::Receiver<RespValue>),
}

/// Helper to aggregate FLUSHDB responses from all workers
async fn aggregate_flushdb_responses(
	flush_rxs: Vec<oneshot::Receiver<RespValue>>,
	final_tx: oneshot::Sender<RespValue>,
) {
	let mut success = true;
	for rx in flush_rxs {
		match rx.await {
			Ok(RespValue::SimpleString(s)) if s == b"OK".as_slice() => {}
			_ => success = false,
		}
	}
	let result = if success {
		RespValue::SimpleString(Bytes::from_static(b"OK"))
	} else {
		RespValue::Error(Bytes::from("Failed to flush all shards"))
	};
	final_tx.send(result).ok();
}

/// Command dispatcher for managing command routing and response collection
pub struct CommandDispatcher {
	peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	batches: HashMap<usize, Vec<CmdRequest>>,
	ordered_responses: Vec<PendingResponse>,
}

impl CommandDispatcher {
	pub fn new(peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>) -> Self {
		Self {
			peers,
			batches: HashMap::new(),
			ordered_responses: Vec::new(),
		}
	}

	pub fn reset(&mut self) {
		self.batches.clear();
		self.ordered_responses.clear();
	}

	/// Core dispatch logic - routes commands based on type
	pub fn dispatch(&mut self, cmd: ParsedCmd) {
		if cmd.name.eq_ignore_ascii_case("flushdb") {
			self.broadcast_cmd(cmd);
		} else {
			self.route_cmd(cmd);
		}
	}

	/// Broadcast command to all workers (FLUSHDB)
	fn broadcast_cmd(&mut self, cmd: ParsedCmd) {
		let (final_tx, final_rx) = oneshot::channel();
		self.ordered_responses
			.push(PendingResponse::Future(final_rx));

		let mut flush_rxs = Vec::with_capacity(self.peers.len());
		for &worker_idx in self.peers.keys() {
			let (tx, rx) = oneshot::channel();
			flush_rxs.push(rx);
			let req = CmdRequest {
				cmd_name: "FLUSHDB".to_string(),
				args: cmd.args.clone(),
				resp_tx: tx,
			};
			self.batches.entry(worker_idx).or_default().push(req);
		}

		tokio::spawn(aggregate_flushdb_responses(flush_rxs, final_tx));
	}

	/// Route command to single worker based on first key's hash (DEL, GET, SET, etc.)
	fn route_cmd(&mut self, cmd: ParsedCmd) {
		let key = cmd.args.first().cloned().unwrap_or_default();
		let target_worker_idx = (hash_key(&key) as usize) % self.peers.len();

		let (resp_tx, resp_rx) = oneshot::channel();
		self.ordered_responses
			.push(PendingResponse::Future(resp_rx));

		let req = CmdRequest {
			cmd_name: cmd.name,
			args: cmd.args,
			resp_tx,
		};

		self.batches.entry(target_worker_idx).or_default().push(req);
	}

	/// Send all batches to respective workers
	pub async fn dispatch_batches(
		&mut self,
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		for (worker_idx, batch) in self.batches.drain() {
			debug!(
				"Dispatching batch of {} cmds to worker {}",
				batch.len(),
				worker_idx
			);
			if let Some(sender) = self.peers.get(&worker_idx) {
				if let Err(e) = sender.send(WorkerMessage::CmdBatch(batch)) {
					error!("Failed to send batch to worker {}: {}", worker_idx, e);
					return Err(e.into());
				}
			} else {
				error!("Worker {} not found", worker_idx);
				return Err("Internal error: worker not found".into());
			}
		}
		Ok(())
	}

	/// Wait for all responses in order and write to socket
	pub async fn await_responses(
		&mut self,
		socket: &mut TcpStream,
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		for response in self.ordered_responses.drain(..) {
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
		Ok(())
	}
}
