use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use fastrace::trace;
use log::debug;
use log::error;
use nimbis_resp::RespEncoder;
use nimbis_resp::RespValue;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::cmd::CmdContext;
use crate::cmd::CmdTable;
use crate::cmd::ParsedCmd;
use crate::cmd::RoutingPolicy;
use crate::worker::CmdRequest;
use crate::worker::WorkerMessage;

/// Simple FNV-1a 64-bit hash for key-based worker routing
/// See: https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function
#[inline]
fn hash_key(key: &[u8]) -> u64 {
	// FNV-1a 64-bit offset basis (standard constant)
	let mut hasher: u64 = 0xcbf29ce484222325;
	for &byte in key {
		hasher ^= byte as u64;
		// FNV-1a 64-bit prime (standard constant)
		hasher = hasher.wrapping_mul(0x100000001b3);
	}
	hasher
}

/// Helper to aggregate FLUSHDB responses from all workers
#[trace]
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
	cmd_table: Arc<CmdTable>,
	ctx: CmdContext,
	batches: HashMap<usize, Vec<CmdRequest>>,
	ordered_responses: Vec<oneshot::Receiver<RespValue>>,
}

impl CommandDispatcher {
	pub fn new(
		peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
		cmd_table: Arc<CmdTable>,
		ctx: CmdContext,
	) -> Self {
		Self {
			peers,
			cmd_table,
			ctx,
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
		let Some(cmd_def) = self.cmd_table.get_cmd(&cmd.name) else {
			self.push_immediate_response(RespValue::error(format!(
				"ERR unknown command '{}'",
				cmd.name.to_lowercase()
			)));
			return;
		};

		match cmd_def.routing() {
			RoutingPolicy::Local => self.route_local(cmd),
			RoutingPolicy::SingleKey => self.route_single_key(cmd),
			RoutingPolicy::MultiKey => self.route_multi_key(cmd),
			RoutingPolicy::Broadcast => self.broadcast_cmd(cmd),
		}
	}

	fn worker_for_key(&self, key: &[u8]) -> usize {
		(hash_key(key) as usize) % self.peers.len()
	}

	fn push_immediate_response(&mut self, resp: RespValue) {
		let (tx, rx) = oneshot::channel();
		if tx.send(resp).is_ok() {
			self.ordered_responses.push(rx);
		}
	}

	fn push_worker_request(&mut self, worker_idx: usize, cmd: ParsedCmd) {
		let (resp_tx, resp_rx) = oneshot::channel();
		self.ordered_responses.push(resp_rx);

		let req = CmdRequest {
			cmd_name: cmd.name,
			args: cmd.args,
			ctx: self.ctx,
			resp_tx,
		};
		self.batches.entry(worker_idx).or_default().push(req);
	}

	/// Route local command to worker 0 for now.
	fn route_local(&mut self, cmd: ParsedCmd) {
		self.push_worker_request(0, cmd);
	}

	/// Broadcast command to all workers (FLUSHDB)
	fn broadcast_cmd(&mut self, cmd: ParsedCmd) {
		let (final_tx, final_rx) = oneshot::channel();
		self.ordered_responses.push(final_rx);

		let mut flush_rxs = Vec::with_capacity(self.peers.len());
		for &worker_idx in self.peers.keys() {
			let (tx, rx) = oneshot::channel();
			flush_rxs.push(rx);
			let req = CmdRequest {
				cmd_name: "FLUSHDB".to_string(),
				args: cmd.args.clone(),
				ctx: self.ctx,
				resp_tx: tx,
			};
			self.batches.entry(worker_idx).or_default().push(req);
		}

		tokio::spawn(aggregate_flushdb_responses(flush_rxs, final_tx));
	}

	/// Route command to single worker based on first key's hash.
	fn route_single_key(&mut self, cmd: ParsedCmd) {
		let Some(key) = cmd.args.first() else {
			self.push_immediate_response(RespValue::error(format!(
				"ERR wrong number of arguments for '{}' command",
				cmd.name.to_lowercase()
			)));
			return;
		};

		let target_worker_idx = self.worker_for_key(key);
		self.push_worker_request(target_worker_idx, cmd);
	}

	fn route_multi_key(&mut self, cmd: ParsedCmd) {
		match cmd.name.as_str() {
			"DEL" | "EXISTS" => self.route_multi_key_sum_integer(cmd),
			_ => self.push_immediate_response(RespValue::error(format!(
				"ERR command '{}' does not support multi-key routing yet",
				cmd.name.to_lowercase()
			))),
		}
	}

	fn route_multi_key_sum_integer(&mut self, cmd: ParsedCmd) {
		if cmd.args.is_empty() {
			self.push_immediate_response(RespValue::error(format!(
				"ERR wrong number of arguments for '{}' command",
				cmd.name.to_lowercase()
			)));
			return;
		}

		let (final_tx, final_rx) = oneshot::channel();
		self.ordered_responses.push(final_rx);

		let mut sub_rxs = Vec::with_capacity(cmd.args.len());
		for key in cmd.args {
			let worker_idx = self.worker_for_key(&key);
			let (resp_tx, resp_rx) = oneshot::channel();
			sub_rxs.push(resp_rx);

			let req = CmdRequest {
				cmd_name: cmd.name.clone(),
				args: vec![key],
				ctx: self.ctx,
				resp_tx,
			};
			self.batches.entry(worker_idx).or_default().push(req);
		}

		tokio::spawn(async move {
			let mut sum = 0_i64;
			for rx in sub_rxs {
				match rx.await {
					Ok(RespValue::Integer(value)) => sum += value,
					Ok(RespValue::Error(err)) => {
						final_tx.send(RespValue::Error(err)).ok();
						return;
					}
					Ok(other) => {
						final_tx
							.send(RespValue::error(format!(
								"ERR unexpected response in multi-key aggregation: {:?}",
								other
							)))
							.ok();
						return;
					}
					Err(_) => {
						final_tx
							.send(RespValue::error("ERR worker dropped multi-key request"))
							.ok();
						return;
					}
				}
			}
			final_tx.send(RespValue::Integer(sum)).ok();
		});
	}

	/// Send all batches to respective workers
	#[trace]
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
	#[trace]
	pub async fn await_responses(
		&mut self,
		socket: &mut TcpStream,
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		for rx in self.ordered_responses.drain(..) {
			debug!("Waiting for response...");
			let resp_value = rx.await.map_err(|_| "worker dropped request")?;
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

#[cfg(test)]
mod tests {
	use std::collections::HashMap;
	use std::sync::Arc;

	use bytes::Bytes;
	use nimbis_resp::RespValue;
	use tokio::sync::mpsc;

	use super::CommandDispatcher;
	use crate::cmd::CmdContext;
	use crate::cmd::CmdTable;
	use crate::cmd::ParsedCmd;
	use crate::worker::CmdRequest;
	use crate::worker::WorkerMessage;

	fn setup_dispatcher() -> (
		CommandDispatcher,
		HashMap<usize, mpsc::UnboundedReceiver<WorkerMessage>>,
	) {
		let mut peers = HashMap::new();
		let mut receivers = HashMap::new();
		for idx in 0..2_usize {
			let (tx, rx) = mpsc::unbounded_channel();
			peers.insert(idx, tx);
			receivers.insert(idx, rx);
		}
		let dispatcher = CommandDispatcher::new(
			Arc::new(peers),
			Arc::new(CmdTable::new()),
			CmdContext { client_id: 42 },
		);
		(dispatcher, receivers)
	}

	fn take_batch(msg: WorkerMessage) -> Vec<CmdRequest> {
		match msg {
			WorkerMessage::CmdBatch(batch) => batch,
			WorkerMessage::NewConnection(_) => panic!("unexpected connection message"),
		}
	}

	#[test]
	fn worker_for_key_is_deterministic() {
		let (dispatcher, _) = setup_dispatcher();
		let key = b"route:key:1";
		let first = dispatcher.worker_for_key(key);
		let second = dispatcher.worker_for_key(key);
		assert_eq!(first, second);
		assert!(first < 2);
	}

	#[tokio::test]
	async fn route_single_key_enqueues_to_expected_worker() {
		let (mut dispatcher, mut receivers) = setup_dispatcher();
		let key = Bytes::from_static(b"single:key");
		let expected_worker = dispatcher.worker_for_key(&key);

		dispatcher.dispatch(ParsedCmd {
			name: "GET".to_string(),
			args: vec![key],
		});
		dispatcher
			.dispatch_batches()
			.await
			.expect("dispatch batches");

		let mut total_reqs = 0usize;
		for idx in 0..2usize {
			let rx = receivers
				.get_mut(&idx)
				.expect("receiver for worker should exist");
			if let Ok(msg) = rx.try_recv() {
				let batch = take_batch(msg);
				total_reqs += batch.len();
				if idx == expected_worker {
					assert_eq!(batch.len(), 1);
					assert_eq!(batch[0].cmd_name, "GET");
				} else {
					assert!(batch.is_empty());
				}
			}
		}
		assert_eq!(total_reqs, 1);
	}

	#[tokio::test]
	async fn multi_key_sum_aggregates_integer_responses() {
		let (mut dispatcher, mut receivers) = setup_dispatcher();
		let keys = vec![
			Bytes::from_static(b"mk:a"),
			Bytes::from_static(b"mk:b"),
			Bytes::from_static(b"mk:c"),
		];

		dispatcher.dispatch(ParsedCmd {
			name: "DEL".to_string(),
			args: keys,
		});
		assert_eq!(dispatcher.ordered_responses.len(), 1);
		dispatcher
			.dispatch_batches()
			.await
			.expect("dispatch batches");

		for idx in 0..2usize {
			let rx = receivers
				.get_mut(&idx)
				.expect("receiver for worker should exist");
			if let Ok(msg) = rx.try_recv() {
				for req in take_batch(msg) {
					req.resp_tx
						.send(RespValue::Integer(1))
						.expect("send integer");
				}
			}
		}

		let aggregated = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("aggregated response");
		assert_eq!(aggregated, RespValue::Integer(3));
	}

	#[tokio::test]
	async fn multi_key_sum_propagates_error() {
		let (mut dispatcher, mut receivers) = setup_dispatcher();
		let keys = vec![Bytes::from_static(b"err:a"), Bytes::from_static(b"err:b")];

		dispatcher.dispatch(ParsedCmd {
			name: "EXISTS".to_string(),
			args: keys,
		});
		dispatcher
			.dispatch_batches()
			.await
			.expect("dispatch batches");

		let mut sent_error = false;
		for idx in 0..2usize {
			let rx = receivers
				.get_mut(&idx)
				.expect("receiver for worker should exist");
			if let Ok(msg) = rx.try_recv() {
				for req in take_batch(msg) {
					if !sent_error {
						req.resp_tx
							.send(RespValue::error("ERR injected error"))
							.expect("send error");
						sent_error = true;
					} else {
						req.resp_tx
							.send(RespValue::Integer(1))
							.expect("send integer");
					}
				}
			}
		}

		let aggregated = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("aggregated response");
		assert!(matches!(aggregated, RespValue::Error(_)));
	}

	#[tokio::test]
	async fn response_order_preserved_with_multi_key_between_single_key() {
		let (mut dispatcher, mut receivers) = setup_dispatcher();

		dispatcher.dispatch(ParsedCmd {
			name: "SET".to_string(),
			args: vec![Bytes::from_static(b"order:k1"), Bytes::from_static(b"v1")],
		});
		dispatcher.dispatch(ParsedCmd {
			name: "DEL".to_string(),
			args: vec![
				Bytes::from_static(b"order:k2"),
				Bytes::from_static(b"order:k3"),
			],
		});
		dispatcher.dispatch(ParsedCmd {
			name: "GET".to_string(),
			args: vec![Bytes::from_static(b"order:k4")],
		});
		assert_eq!(dispatcher.ordered_responses.len(), 3);
		dispatcher
			.dispatch_batches()
			.await
			.expect("dispatch batches");

		for idx in 0..2usize {
			let rx = receivers
				.get_mut(&idx)
				.expect("receiver for worker should exist");
			if let Ok(msg) = rx.try_recv() {
				for req in take_batch(msg) {
					match req.cmd_name.as_str() {
						"SET" => req
							.resp_tx
							.send(RespValue::simple_string("OK"))
							.expect("send set resp"),
						"DEL" => req
							.resp_tx
							.send(RespValue::Integer(1))
							.expect("send del resp"),
						"GET" => req
							.resp_tx
							.send(RespValue::bulk_string("v4"))
							.expect("send get resp"),
						other => panic!("unexpected command {}", other),
					}
				}
			}
		}

		let first = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("first response");
		let second = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("second response");
		let third = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("third response");

		assert_eq!(first, RespValue::simple_string("OK"));
		assert_eq!(second, RespValue::Integer(2));
		assert_eq!(third, RespValue::bulk_string("v4"));
	}
}
