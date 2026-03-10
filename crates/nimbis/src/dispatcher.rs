use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use log::debug;
use log::error;
use resp::RespEncoder;
use resp::RespValue;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::cmd::ParsedCmd;
use crate::worker::CmdRequest;
use crate::worker::WorkerMessage;

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
const VIRTUAL_NODES_PER_WORKER: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VirtualNode {
	hash: u64,
	worker_idx: usize,
}

#[derive(Debug)]
pub(crate) struct HashRing {
	nodes: Vec<VirtualNode>,
}

impl HashRing {
	pub(crate) fn new<I>(worker_indices: I) -> Self
	where
		I: IntoIterator<Item = usize>,
	{
		Self::with_virtual_nodes(worker_indices, VIRTUAL_NODES_PER_WORKER)
	}

	fn with_virtual_nodes<I>(worker_indices: I, virtual_nodes_per_worker: usize) -> Self
	where
		I: IntoIterator<Item = usize>,
	{
		let mut nodes = Vec::new();
		for worker_idx in worker_indices {
			for virtual_node_idx in 0..virtual_nodes_per_worker {
				nodes.push(VirtualNode {
					hash: hash_virtual_node(worker_idx, virtual_node_idx),
					worker_idx,
				});
			}
		}

		nodes.sort_unstable_by_key(|node| node.hash);
		Self { nodes }
	}

	fn worker_for_key(&self, key: &[u8]) -> usize {
		debug_assert!(
			!self.nodes.is_empty(),
			"hash ring must contain at least one virtual node"
		);

		let key_hash = hash_key(key);
		let idx = match self.nodes.binary_search_by_key(&key_hash, |node| node.hash) {
			Ok(idx) | Err(idx) => idx,
		};
		self.nodes[idx % self.nodes.len()].worker_idx
	}
}

#[inline]
fn fnv1a_extend(mut hasher: u64, bytes: &[u8]) -> u64 {
	for &byte in bytes {
		hasher ^= byte as u64;
		hasher = hasher.wrapping_mul(FNV_PRIME);
	}
	hasher
}

#[inline]
fn mix64(mut value: u64) -> u64 {
	value ^= value >> 33;
	value = value.wrapping_mul(0xff51afd7ed558ccd);
	value ^= value >> 33;
	value = value.wrapping_mul(0xc4ceb9fe1a85ec53);
	value ^ (value >> 33)
}

/// Simple FNV-1a 64-bit hash for key-based worker routing
/// See: https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function
#[inline]
fn hash_key(key: &[u8]) -> u64 {
	mix64(fnv1a_extend(FNV_OFFSET_BASIS, key))
}

#[inline]
fn hash_virtual_node(worker_idx: usize, virtual_node_idx: usize) -> u64 {
	hash_key(format!("worker-{worker_idx}-vnode-{virtual_node_idx}").as_bytes())
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
	hash_ring: Arc<HashRing>,
	batches: HashMap<usize, Vec<CmdRequest>>,
	ordered_responses: Vec<oneshot::Receiver<RespValue>>,
}

impl CommandDispatcher {
	pub(crate) fn new(
		peers: Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
		hash_ring: Arc<HashRing>,
	) -> Self {
		Self {
			peers,
			hash_ring,
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
		self.ordered_responses.push(final_rx);

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

	/// Route command to single worker based on first key's hash (DEL, GET, SET,
	/// etc.)
	fn route_cmd(&mut self, cmd: ParsedCmd) {
		let key = cmd.args.first().cloned().unwrap_or_default();
		let target_worker_idx = self.hash_ring.worker_for_key(&key);

		let (resp_tx, resp_rx) = oneshot::channel();
		self.ordered_responses.push(resp_rx);

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
	use super::HashRing;
	use super::VIRTUAL_NODES_PER_WORKER;

	fn distribution_counts(ring: &HashRing, workers: usize, key_count: usize) -> Vec<usize> {
		let mut counts = vec![0; workers];
		for i in 0..key_count {
			let key = format!("key-{i}");
			let worker_idx = ring.worker_for_key(key.as_bytes());
			counts[worker_idx] += 1;
		}
		counts
	}

	fn spread(counts: &[usize]) -> usize {
		counts.iter().copied().max().unwrap() - counts.iter().copied().min().unwrap()
	}

	#[test]
	fn routes_same_key_to_same_worker() {
		let ring = HashRing::new(0..4);
		let first = ring.worker_for_key(b"user:42");

		for _ in 0..16 {
			assert_eq!(ring.worker_for_key(b"user:42"), first);
		}
	}

	#[test]
	fn virtual_nodes_improve_distribution() {
		let workers = 8;
		let key_count = 50_000;
		let single_node_ring = HashRing::with_virtual_nodes(0..workers, 1);
		let virtual_node_ring = HashRing::with_virtual_nodes(0..workers, VIRTUAL_NODES_PER_WORKER);

		let single_counts = distribution_counts(&single_node_ring, workers, key_count);
		let virtual_counts = distribution_counts(&virtual_node_ring, workers, key_count);

		assert!(
			spread(&virtual_counts) < spread(&single_counts),
			"virtual nodes should improve balance, single={single_counts:?}, virtual={virtual_counts:?}"
		);

		let expected = key_count as f64 / workers as f64;
		let max_deviation = virtual_counts
			.iter()
			.map(|&count| ((count as f64 - expected).abs()) / expected)
			.fold(0.0, f64::max);

		assert!(
			max_deviation < 0.20,
			"virtual node distribution is still too uneven: counts={virtual_counts:?}"
		);
	}
}
