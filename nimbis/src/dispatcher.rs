use std::collections::HashMap;
use std::sync::Arc;

use fastrace::trace;
use log::debug;
use log::error;
use nimbis_resp::RespEncoder;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::cmd::CmdContext;
use crate::cmd::CmdTable;
use crate::cmd::CommandKind;
use crate::cmd::ParsedCmd;
use crate::key_hash::hash_key;
use crate::worker::CmdRequest;
use crate::worker::WorkerMessage;

/// Command dispatcher for managing command routing and response collection
pub struct CommandDispatcher {
	peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	cmd_table: Arc<CmdTable>,
	ctx: CmdContext,
	local_tx: mpsc::UnboundedSender<(ParsedCmd, oneshot::Sender<RespValue>)>,
	batches: HashMap<usize, Vec<CmdRequest>>,
	ordered_responses: Vec<oneshot::Receiver<RespValue>>,
}

impl CommandDispatcher {
	pub fn new(
		peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
		cmd_table: Arc<CmdTable>,
		storage: Arc<Storage>,
		ctx: CmdContext,
	) -> Self {
		let (local_tx, mut local_rx) =
			mpsc::unbounded_channel::<(ParsedCmd, oneshot::Sender<RespValue>)>();
		let cmd_table_clone = cmd_table.clone();
		let storage_clone = storage.clone();

		tokio::spawn(async move {
			while let Some((cmd, tx)) = local_rx.recv().await {
				let response = match cmd_table_clone.get_cmd(&cmd.name) {
					Some(cmd_def) => cmd_def.execute(&storage_clone, &cmd.args, &ctx).await,
					None => RespValue::error(format!(
						"ERR unknown command '{}'",
						cmd.name.to_lowercase()
					)),
				};
				tx.send(response).ok();
			}
		});

		Self {
			peers,
			cmd_table,
			ctx,
			local_tx,
			batches: HashMap::new(),
			ordered_responses: Vec::new(),
		}
	}

	pub fn reset(&mut self) {
		self.batches.clear();
		self.ordered_responses.clear();
	}

	/// Core dispatch logic - routes commands from command metadata.
	pub async fn dispatch(
		&mut self,
		cmd: ParsedCmd,
	) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let Some(cmd_def) = self.cmd_table.get_cmd(&cmd.name) else {
			self.push_immediate_response(RespValue::error(format!(
				"ERR unknown command '{}'",
				cmd.name.to_lowercase()
			)));
			return Ok(());
		};

		if let Err(err) = cmd_def.meta().validate_arity(cmd.args.len() + 1) {
			self.push_immediate_response(RespValue::error(err));
			return Ok(());
		}

		match cmd_def.meta().kind {
			CommandKind::Local => self.execute_local(cmd),
			CommandKind::Admin => self.broadcast(cmd),
			CommandKind::Read | CommandKind::Write => {
				let keys = match cmd_def.meta().keys(&cmd.args) {
					Ok(keys) => keys,
					Err(resp) => {
						self.push_immediate_response(resp);
						return Ok(());
					}
				};
				let Some(first_key) = keys.first() else {
					self.push_immediate_response(RespValue::error(format!(
						"ERR wrong number of arguments for '{}' command",
						cmd.name.to_lowercase()
					)));
					return Ok(());
				};

				let worker_idx = self.worker_for_key(first_key);
				if keys
					.iter()
					.any(|key| self.worker_for_key(key) != worker_idx)
				{
					let message = if cmd_def.meta().is_write() {
						"ERR cross-shard write command is not supported"
					} else {
						"ERR cross-shard command is not supported"
					};
					self.push_immediate_response(RespValue::error(message));
					return Ok(());
				}

				self.push_worker_request(worker_idx, cmd);
			}
		}

		Ok(())
	}

	fn execute_local(&mut self, request: ParsedCmd) {
		let (tx, rx) = oneshot::channel();
		self.ordered_responses.push(rx);
		self.local_tx.send((request, tx)).ok();
	}

	fn worker_for_key(&self, key: &[u8]) -> usize {
		(hash_key(key) as usize) % self.peers.len()
	}

	fn push_worker_request(&mut self, worker_idx: usize, cmd: ParsedCmd) {
		let (resp_tx, resp_rx) = oneshot::channel();
		self.ordered_responses.push(resp_rx);
		self.push_subrequest(worker_idx, cmd, resp_tx);
	}

	fn push_subrequest(
		&mut self,
		worker_idx: usize,
		cmd: ParsedCmd,
		resp_tx: oneshot::Sender<RespValue>,
	) {
		let req = CmdRequest {
			cmd_name: cmd.name,
			args: cmd.args,
			ctx: self.ctx,
			resp_tx,
		};
		self.batches.entry(worker_idx).or_default().push(req);
	}

	fn broadcast(&mut self, cmd: ParsedCmd) {
		let (final_tx, final_rx) = oneshot::channel();
		self.ordered_responses.push(final_rx);

		let mut sub_rxs = Vec::with_capacity(self.peers.len());
		let worker_ids: Vec<_> = self.peers.keys().copied().collect();
		for worker_idx in worker_ids {
			let (tx, rx) = oneshot::channel();
			sub_rxs.push(rx);
			self.push_subrequest(worker_idx, cmd.clone(), tx);
		}

		tokio::spawn(async move {
			for rx in sub_rxs {
				match rx.await {
					Ok(RespValue::SimpleString(s)) if s == b"OK".as_slice() => {}
					Ok(RespValue::Error(err)) => {
						final_tx.send(RespValue::Error(err)).ok();
						return;
					}
					Ok(other) => {
						final_tx
							.send(RespValue::error(format!(
								"ERR unexpected response in broadcast aggregation: {:?}",
								other
							)))
							.ok();
						return;
					}
					Err(_) => {
						final_tx
							.send(RespValue::error("ERR worker dropped broadcast request"))
							.ok();
						return;
					}
				}
			}
			final_tx.send(RespValue::simple_string("OK")).ok();
		});
	}

	fn push_immediate_response(&mut self, resp: RespValue) {
		let (tx, rx) = oneshot::channel();
		if tx.send(resp).is_ok() {
			self.ordered_responses.push(rx);
		}
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
	use std::env;
	use std::sync::Arc;
	use std::sync::atomic::AtomicU64;
	use std::sync::atomic::Ordering;
	use std::time::Duration;
	use std::time::SystemTime;
	use std::time::UNIX_EPOCH;

	use bytes::Bytes;
	use nimbis_resp::RespValue;
	use nimbis_storage::Storage;
	use tokio::sync::mpsc;

	use super::CommandDispatcher;
	use crate::client::ClientSessions;
	use crate::cmd::CmdContext;
	use crate::cmd::CmdTable;
	use crate::cmd::ParsedCmd;
	use crate::context::init_global_context;
	use crate::worker::CmdRequest;
	use crate::worker::WorkerMessage;

	static TEST_STORAGE_ID: AtomicU64 = AtomicU64::new(0);

	async fn create_test_storage() -> Arc<Storage> {
		let unique = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.expect("system time before epoch")
			.as_nanos();
		let id = TEST_STORAGE_ID.fetch_add(1, Ordering::Relaxed);
		let path = env::temp_dir().join(format!("nimbis-dispatcher-test-{unique}-{id}"));
		Arc::new(
			Storage::open(&path, Some(0))
				.await
				.expect("create test storage"),
		)
	}

	async fn setup_dispatcher() -> (
		CommandDispatcher,
		HashMap<usize, mpsc::UnboundedReceiver<WorkerMessage>>,
	) {
		init_global_context(Arc::new(ClientSessions::new()));
		let mut peers = HashMap::new();
		let mut receivers = HashMap::new();
		for idx in 0..2_usize {
			let (tx, rx) = mpsc::unbounded_channel();
			peers.insert(idx, tx);
			receivers.insert(idx, rx);
		}
		let storage = create_test_storage().await;
		let dispatcher = CommandDispatcher::new(
			Arc::new(peers),
			Arc::new(CmdTable::new()),
			storage,
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

	#[tokio::test]
	async fn worker_for_key_is_deterministic() {
		let (dispatcher, _) = setup_dispatcher().await;
		let key = b"route:key:1";
		let first = dispatcher.worker_for_key(key);
		let second = dispatcher.worker_for_key(key);
		assert_eq!(first, second);
		assert!(first < 2);
	}

	#[tokio::test]
	async fn route_single_key_enqueues_to_expected_worker() {
		let (mut dispatcher, mut receivers) = setup_dispatcher().await;
		let key = Bytes::from_static(b"single:key");
		let expected_worker = dispatcher.worker_for_key(&key);

		dispatcher
			.dispatch(ParsedCmd {
				name: "GET".to_string(),
				args: vec![key],
			})
			.await
			.expect("dispatch");
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
	async fn mset_groups_same_worker_keys_into_one_request() {
		let (mut dispatcher, mut receivers) = setup_dispatcher().await;
		let mut keys_by_worker: HashMap<usize, Vec<Bytes>> = HashMap::new();
		for idx in 0..100usize {
			let key = Bytes::from(format!("mset:same-worker:{idx}"));
			let worker = dispatcher.worker_for_key(&key);
			let keys = keys_by_worker.entry(worker).or_default();
			keys.push(key);
			if keys.len() == 2 {
				break;
			}
		}

		let (expected_worker, keys) = keys_by_worker
			.into_iter()
			.find(|(_, keys)| keys.len() == 2)
			.expect("find keys on the same worker");
		let key1 = keys[0].clone();
		let key2 = keys[1].clone();
		let expected_args = vec![
			key1.clone(),
			Bytes::from_static(b"v1"),
			key2.clone(),
			Bytes::from_static(b"v2"),
		];

		dispatcher
			.dispatch(ParsedCmd {
				name: "MSET".to_string(),
				args: expected_args.clone(),
			})
			.await
			.expect("dispatch");
		dispatcher
			.dispatch_batches()
			.await
			.expect("dispatch batches");

		let rx = receivers
			.get_mut(&expected_worker)
			.expect("receiver for expected worker should exist");
		let msg = tokio::time::timeout(Duration::from_millis(100), rx.recv())
			.await
			.expect("MSET should reach expected worker")
			.expect("worker message should exist");
		let mut batch = take_batch(msg);
		assert_eq!(batch.len(), 1);
		assert_eq!(batch[0].cmd_name, "MSET");
		assert_eq!(batch[0].args.as_slice(), expected_args.as_slice());
		batch
			.pop()
			.expect("mset request")
			.resp_tx
			.send(RespValue::simple_string("OK"))
			.expect("send mset response");

		for idx in 0..2usize {
			if idx != expected_worker {
				let rx = receivers
					.get_mut(&idx)
					.expect("receiver for worker should exist");
				assert!(rx.try_recv().is_err());
			}
		}

		let response = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("MSET response");
		assert_eq!(response, RespValue::simple_string("OK"));
	}

	#[tokio::test]
	async fn same_worker_mset_preserves_batch_order_after_single_key() {
		let (mut dispatcher, mut receivers) = setup_dispatcher().await;
		let mut keys_by_worker: HashMap<usize, Vec<Bytes>> = HashMap::new();
		for idx in 0..200usize {
			let key = Bytes::from(format!("locked-order:{idx}"));
			let worker = dispatcher.worker_for_key(&key);
			let keys = keys_by_worker.entry(worker).or_default();
			keys.push(key);
			if keys.len() == 3 {
				break;
			}
		}

		let (expected_worker, keys) = keys_by_worker
			.into_iter()
			.find(|(_, keys)| keys.len() == 3)
			.expect("find three keys on the same worker");
		let pre_key = keys[0].clone();
		let key1 = keys[1].clone();
		let key2 = keys[2].clone();

		dispatcher
			.dispatch(ParsedCmd {
				name: "SET".to_string(),
				args: vec![pre_key.clone(), Bytes::from_static(b"pre")],
			})
			.await
			.expect("dispatch queued set");

		dispatcher
			.dispatch(ParsedCmd {
				name: "MSET".to_string(),
				args: vec![
					key1.clone(),
					Bytes::from_static(b"v1"),
					key2.clone(),
					Bytes::from_static(b"v2"),
				],
			})
			.await
			.expect("dispatch mset");
		dispatcher
			.dispatch_batches()
			.await
			.expect("dispatch batches");

		let rx = receivers
			.get_mut(&expected_worker)
			.expect("receiver for expected worker should exist");
		let first_msg = tokio::time::timeout(Duration::from_millis(100), rx.recv())
			.await
			.expect("worker batch should arrive")
			.expect("first worker message should exist");
		let batch = take_batch(first_msg);
		assert_eq!(batch.len(), 2);
		assert_eq!(batch[0].cmd_name, "SET");
		assert_eq!(batch[0].args[0], pre_key);
		assert_eq!(batch[1].cmd_name, "MSET");

		for req in batch {
			req.resp_tx
				.send(RespValue::simple_string("OK"))
				.expect("send response");
		}

		let first_response = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("set response");
		let second_response = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("mset response");
		assert_eq!(first_response, RespValue::simple_string("OK"));
		assert_eq!(second_response, RespValue::simple_string("OK"));
	}

	#[tokio::test]
	async fn multi_key_command_routes_to_single_owner_when_keys_share_worker() {
		let (mut dispatcher, mut receivers) = setup_dispatcher().await;
		let mut keys_by_worker: HashMap<usize, Vec<Bytes>> = HashMap::new();
		for idx in 0..200usize {
			let key = Bytes::from(format!("same-worker-del:{idx}"));
			let worker = dispatcher.worker_for_key(&key);
			let keys = keys_by_worker.entry(worker).or_default();
			keys.push(key);
			if keys.len() == 3 {
				break;
			}
		}
		let (expected_worker, keys) = keys_by_worker
			.into_iter()
			.find(|(_, keys)| keys.len() == 3)
			.expect("find keys on same worker");

		dispatcher
			.dispatch(ParsedCmd {
				name: "DEL".to_string(),
				args: keys.clone(),
			})
			.await
			.expect("dispatch");
		assert_eq!(dispatcher.ordered_responses.len(), 1);
		dispatcher
			.dispatch_batches()
			.await
			.expect("dispatch batches");

		let rx = receivers
			.get_mut(&expected_worker)
			.expect("receiver for worker should exist");
		let msg = rx.try_recv().expect("worker batch");
		let mut batch = take_batch(msg);
		assert_eq!(batch.len(), 1);
		assert_eq!(batch[0].cmd_name, "DEL");
		assert_eq!(batch[0].args, keys);
		batch
			.pop()
			.expect("del request")
			.resp_tx
			.send(RespValue::Integer(3))
			.expect("send integer");

		for idx in 0..2usize {
			if idx != expected_worker {
				let rx = receivers
					.get_mut(&idx)
					.expect("receiver for worker should exist");
				assert!(rx.try_recv().is_err());
			}
		}

		let response = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("response");
		assert_eq!(response, RespValue::Integer(3));
	}

	#[tokio::test]
	async fn multi_key_command_rejects_cross_worker_keys() {
		let (mut dispatcher, _) = setup_dispatcher().await;
		let mut seen_by_worker: HashMap<usize, Bytes> = HashMap::new();
		let mut keys = None;
		for idx in 0..200usize {
			let key = Bytes::from(format!("cross-worker-exists:{idx}"));
			let worker = dispatcher.worker_for_key(&key);
			for (other_worker, other_key) in &seen_by_worker {
				if *other_worker != worker {
					keys = Some(vec![other_key.clone(), key.clone()]);
					break;
				}
			}
			if keys.is_some() {
				break;
			}
			seen_by_worker.entry(worker).or_insert(key);
		}
		let keys = keys.expect("find cross-worker keys");

		dispatcher
			.dispatch(ParsedCmd {
				name: "EXISTS".to_string(),
				args: keys,
			})
			.await
			.expect("dispatch");

		let response = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("response");
		assert_eq!(
			response,
			RespValue::error("ERR cross-shard command is not supported")
		);
	}

	#[tokio::test]
	async fn response_order_preserved_with_multi_key_between_single_key() {
		let (mut dispatcher, mut receivers) = setup_dispatcher().await;
		let mut keys_by_worker: HashMap<usize, Vec<Bytes>> = HashMap::new();
		for idx in 0..200usize {
			let key = Bytes::from(format!("order-same-worker:{idx}"));
			let worker = dispatcher.worker_for_key(&key);
			let keys = keys_by_worker.entry(worker).or_default();
			keys.push(key);
			if keys.len() == 2 {
				break;
			}
		}
		let del_keys = keys_by_worker
			.into_iter()
			.find(|(_, keys)| keys.len() == 2)
			.expect("find del keys on same worker")
			.1;

		dispatcher
			.dispatch(ParsedCmd {
				name: "SET".to_string(),
				args: vec![Bytes::from_static(b"order:k1"), Bytes::from_static(b"v1")],
			})
			.await
			.expect("dispatch");
		dispatcher
			.dispatch(ParsedCmd {
				name: "DEL".to_string(),
				args: del_keys,
			})
			.await
			.expect("dispatch");
		dispatcher
			.dispatch(ParsedCmd {
				name: "GET".to_string(),
				args: vec![Bytes::from_static(b"order:k4")],
			})
			.await
			.expect("dispatch");
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
							.send(RespValue::Integer(2))
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

	#[tokio::test]
	async fn local_command_does_not_enqueue_worker_batch() {
		let (mut dispatcher, mut receivers) = setup_dispatcher().await;

		dispatcher
			.dispatch(ParsedCmd {
				name: "PING".to_string(),
				args: vec![],
			})
			.await
			.expect("dispatch");
		dispatcher
			.dispatch_batches()
			.await
			.expect("dispatch batches");

		for idx in 0..2usize {
			let rx = receivers
				.get_mut(&idx)
				.expect("receiver for worker should exist");
			assert!(
				rx.try_recv().is_err(),
				"local command should not be routed to worker {idx}"
			);
		}

		let resp = dispatcher
			.ordered_responses
			.remove(0)
			.await
			.expect("local response");
		assert_eq!(resp, RespValue::simple_string("PONG"));
	}
}
