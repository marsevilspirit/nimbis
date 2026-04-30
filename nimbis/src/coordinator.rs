use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use bytes::Bytes;
use nimbis_resp::RespValue;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::GCTX;
use crate::cmd::CmdContext;
use crate::cmd::ParsedCmd;
use crate::worker::CmdRequest;
use crate::worker::WorkerMessage;

/// Simple FNV-1a 64-bit hash for key-based worker routing.
/// See: https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function
#[inline]
pub(crate) fn hash_key(key: &[u8]) -> u64 {
	let mut hasher: u64 = 0xcbf29ce484222325;
	for &byte in key {
		hasher ^= byte as u64;
		hasher = hasher.wrapping_mul(0x100000001b3);
	}
	hasher
}

#[derive(Debug, Clone)]
pub enum CommandPlan {
	Local {
		request: ParsedCmd,
	},
	SingleKey {
		key: Bytes,
		request: ParsedCmd,
	},
	Broadcast {
		request: ParsedCmd,
	},
	Scatter {
		subrequests: Vec<ScatterRequest>,
		aggregate: AggregatePolicy,
	},
	LockedMultiKey {
		keys: Vec<Bytes>,
		execution: LockedExecution,
	},
}

#[derive(Debug, Clone)]
pub struct ScatterRequest {
	pub route_key: Bytes,
	pub request: ParsedCmd,
	pub output_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregatePolicy {
	IntegerSum,
	OrderedArray,
	AllOk,
	SetUnion,
	SetIntersection,
	SetDifference,
}

#[derive(Debug, Clone)]
pub enum LockedExecution {
	SameCommandOnOwner {
		request: ParsedCmd,
	},
	GroupedByShard {
		requests: Vec<ScatterRequest>,
		aggregate: AggregatePolicy,
	},
	MSetNx {
		pairs: Vec<(Bytes, Bytes)>,
	},
}

/// Executes command plans while leaving command semantics out of the
/// dispatcher.
pub struct MultiKeyCoordinator<'a> {
	peers: &'a Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	ctx: CmdContext,
	batches: &'a mut HashMap<usize, Vec<CmdRequest>>,
	ordered_responses: &'a mut Vec<oneshot::Receiver<RespValue>>,
}

impl<'a> MultiKeyCoordinator<'a> {
	pub fn new(
		peers: &'a Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
		ctx: CmdContext,
		batches: &'a mut HashMap<usize, Vec<CmdRequest>>,
		ordered_responses: &'a mut Vec<oneshot::Receiver<RespValue>>,
	) -> Self {
		Self {
			peers,
			ctx,
			batches,
			ordered_responses,
		}
	}

	pub fn worker_for_key(&self, key: &[u8]) -> usize {
		(hash_key(key) as usize) % self.peers.len()
	}

	pub fn execute(&mut self, plan: CommandPlan) {
		match plan {
			CommandPlan::Local { request } => self.push_worker_request(0, request),
			CommandPlan::SingleKey { key, request } => {
				let worker_idx = self.worker_for_key(&key);
				self.push_worker_request(worker_idx, request);
			}
			CommandPlan::Broadcast { request } => self.broadcast(request),
			CommandPlan::Scatter {
				subrequests,
				aggregate,
			} => self.scatter(subrequests, aggregate),
			CommandPlan::LockedMultiKey { keys, execution } => self.locked(keys, execution),
		}
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
		for &worker_idx in self.peers.keys() {
			let (tx, rx) = oneshot::channel();
			sub_rxs.push((None, rx));
			self.push_subrequest(worker_idx, cmd.clone(), tx);
		}

		tokio::spawn(async move {
			final_tx
				.send(aggregate_responses(sub_rxs, AggregatePolicy::AllOk).await)
				.ok();
		});
	}

	fn scatter(&mut self, subrequests: Vec<ScatterRequest>, aggregate: AggregatePolicy) {
		let (final_tx, final_rx) = oneshot::channel();
		self.ordered_responses.push(final_rx);

		let mut sub_rxs = Vec::with_capacity(subrequests.len());
		for subrequest in subrequests {
			let worker_idx = self.worker_for_key(&subrequest.route_key);
			let (tx, rx) = oneshot::channel();
			sub_rxs.push((subrequest.output_index, rx));
			self.push_subrequest(worker_idx, subrequest.request, tx);
		}

		tokio::spawn(async move {
			final_tx
				.send(aggregate_responses(sub_rxs, aggregate).await)
				.ok();
		});
	}

	fn locked(&mut self, keys: Vec<Bytes>, execution: LockedExecution) {
		let (final_tx, final_rx) = oneshot::channel();
		self.ordered_responses.push(final_rx);

		let peers = self.peers.clone();
		let ctx = self.ctx;
		tokio::spawn(async move {
			let _guard = GCTX!(key_locks).lock_keys(&keys).await;
			let response = execute_locked(&peers, ctx, execution).await;
			final_tx.send(response).ok();
		});
	}
}

async fn execute_locked(
	peers: &Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	ctx: CmdContext,
	execution: LockedExecution,
) -> RespValue {
	match execution {
		LockedExecution::SameCommandOnOwner { request } => {
			let worker_idx = worker_for_key(peers, request.args.first().map_or(&[], Bytes::as_ref));
			send_worker_request(peers, worker_idx, request, ctx).await
		}
		LockedExecution::GroupedByShard {
			requests,
			aggregate,
		} => {
			let mut rxs = Vec::with_capacity(requests.len());
			for request in requests {
				let worker_idx = worker_for_key(peers, &request.route_key);
				let output_index = request.output_index;
				let rx = send_worker_request_rx(peers, worker_idx, request.request, ctx);
				rxs.push((output_index, rx));
			}
			aggregate_responses(rxs, aggregate).await
		}
		LockedExecution::MSetNx { pairs } => execute_locked_msetnx(peers, ctx, pairs).await,
	}
}

async fn execute_locked_msetnx(
	peers: &Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	ctx: CmdContext,
	pairs: Vec<(Bytes, Bytes)>,
) -> RespValue {
	let mut exists_rxs = Vec::with_capacity(pairs.len());
	for (key, _) in &pairs {
		let request = ParsedCmd {
			name: "EXISTS".to_string(),
			args: vec![key.clone()],
		};
		let worker_idx = worker_for_key(peers, key);
		exists_rxs.push((
			None,
			send_worker_request_rx(peers, worker_idx, request, ctx),
		));
	}

	match aggregate_responses(exists_rxs, AggregatePolicy::IntegerSum).await {
		RespValue::Integer(0) => {}
		RespValue::Integer(_) => return RespValue::Integer(0),
		err @ RespValue::Error(_) => return err,
		other => {
			return RespValue::error(format!(
				"ERR unexpected response in MSETNX existence check: {:?}",
				other
			));
		}
	}

	let grouped = group_pairs_by_worker(peers, pairs);
	let mut write_rxs = Vec::with_capacity(grouped.len());
	for (worker_idx, args) in grouped {
		let request = ParsedCmd {
			name: "MSET".to_string(),
			args,
		};
		write_rxs.push((
			None,
			send_worker_request_rx(peers, worker_idx, request, ctx),
		));
	}

	match aggregate_responses(write_rxs, AggregatePolicy::AllOk).await {
		RespValue::SimpleString(ok) if ok == b"OK".as_slice() => RespValue::Integer(1),
		err @ RespValue::Error(_) => err,
		other => RespValue::error(format!(
			"ERR unexpected response in MSETNX write: {:?}",
			other
		)),
	}
}

fn group_pairs_by_worker(
	peers: &Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	pairs: Vec<(Bytes, Bytes)>,
) -> HashMap<usize, Vec<Bytes>> {
	let mut grouped = HashMap::new();
	for (key, value) in pairs {
		grouped
			.entry(worker_for_key(peers, &key))
			.or_insert_with(Vec::new)
			.extend([key, value]);
	}
	grouped
}

fn worker_for_key(
	peers: &Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	key: &[u8],
) -> usize {
	(hash_key(key) as usize) % peers.len()
}

fn send_worker_request_rx(
	peers: &Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	worker_idx: usize,
	cmd: ParsedCmd,
	ctx: CmdContext,
) -> oneshot::Receiver<RespValue> {
	let (resp_tx, resp_rx) = oneshot::channel();
	let response_on_send_error = RespValue::error(format!("ERR worker {} unavailable", worker_idx));

	match peers.get(&worker_idx) {
		Some(sender) => {
			let req = CmdRequest {
				cmd_name: cmd.name,
				args: cmd.args,
				ctx,
				resp_tx,
			};
			if let Err(err) = sender.send(WorkerMessage::CmdBatch(vec![req])) {
				let req = err.0.into_cmd_batch().into_iter().next();
				if let Some(req) = req {
					req.resp_tx.send(response_on_send_error).ok();
				}
			}
		}
		None => {
			resp_tx.send(response_on_send_error).ok();
		}
	}

	resp_rx
}

async fn send_worker_request(
	peers: &Arc<HashMap<usize, mpsc::UnboundedSender<WorkerMessage>>>,
	worker_idx: usize,
	cmd: ParsedCmd,
	ctx: CmdContext,
) -> RespValue {
	match send_worker_request_rx(peers, worker_idx, cmd, ctx).await {
		Ok(resp) => resp,
		Err(_) => RespValue::error("ERR worker dropped multi-key request"),
	}
}

trait IntoCmdBatch {
	fn into_cmd_batch(self) -> Vec<CmdRequest>;
}

impl IntoCmdBatch for WorkerMessage {
	fn into_cmd_batch(self) -> Vec<CmdRequest> {
		match self {
			WorkerMessage::CmdBatch(batch) => batch,
			WorkerMessage::NewConnection(_) => Vec::new(),
		}
	}
}

async fn aggregate_responses(
	sub_rxs: Vec<(Option<usize>, oneshot::Receiver<RespValue>)>,
	policy: AggregatePolicy,
) -> RespValue {
	let mut responses = Vec::with_capacity(sub_rxs.len());
	for (output_index, rx) in sub_rxs {
		match rx.await {
			Ok(RespValue::Error(err)) => return RespValue::Error(err),
			Ok(resp) => responses.push((output_index, resp)),
			Err(_) => return RespValue::error("ERR worker dropped multi-key request"),
		}
	}

	match policy {
		AggregatePolicy::IntegerSum => aggregate_integer_sum(responses),
		AggregatePolicy::OrderedArray => aggregate_ordered_array(responses),
		AggregatePolicy::AllOk => aggregate_all_ok(responses),
		AggregatePolicy::SetUnion => aggregate_set_union(responses),
		AggregatePolicy::SetIntersection => aggregate_set_intersection(responses),
		AggregatePolicy::SetDifference => aggregate_set_difference(responses),
	}
}

fn aggregate_integer_sum(responses: Vec<(Option<usize>, RespValue)>) -> RespValue {
	let mut sum = 0_i64;
	for (_, resp) in responses {
		match resp {
			RespValue::Integer(value) => sum += value,
			other => return unexpected_aggregate_response(other),
		}
	}
	RespValue::Integer(sum)
}

fn aggregate_ordered_array(responses: Vec<(Option<usize>, RespValue)>) -> RespValue {
	let len = responses
		.iter()
		.filter_map(|(idx, _)| *idx)
		.max()
		.map_or(0, |idx| idx + 1);
	let mut values = vec![RespValue::Null; len];

	for (idx, resp) in responses {
		let Some(idx) = idx else {
			return unexpected_aggregate_response(resp);
		};
		values[idx] = resp;
	}

	RespValue::Array(values)
}

fn aggregate_all_ok(responses: Vec<(Option<usize>, RespValue)>) -> RespValue {
	for (_, resp) in responses {
		match resp {
			RespValue::SimpleString(s) if s == b"OK".as_slice() => {}
			other => return unexpected_aggregate_response(other),
		}
	}
	RespValue::SimpleString(Bytes::from_static(b"OK"))
}

fn aggregate_set_union(responses: Vec<(Option<usize>, RespValue)>) -> RespValue {
	let mut result = HashSet::new();
	for (_, resp) in responses {
		match members_from_response(resp) {
			Ok(members) => {
				for member in members {
					result.insert(member);
				}
			}
			Err(err) => return err,
		}
	}
	bulk_array_from_set(result)
}

fn aggregate_set_intersection(responses: Vec<(Option<usize>, RespValue)>) -> RespValue {
	let mut sets = Vec::new();
	for (_, resp) in responses {
		match members_from_response(resp) {
			Ok(members) => sets.push(members),
			Err(err) => return err,
		}
	}

	let Some(first) = sets.first().cloned() else {
		return RespValue::Array(Vec::new());
	};

	let result = sets
		.into_iter()
		.skip(1)
		.fold(first, |acc, set| acc.intersection(&set).cloned().collect());
	bulk_array_from_set(result)
}

fn aggregate_set_difference(responses: Vec<(Option<usize>, RespValue)>) -> RespValue {
	let mut sets_by_index = HashMap::new();
	for (idx, resp) in responses {
		let Some(idx) = idx else {
			return unexpected_aggregate_response(resp);
		};
		match members_from_response(resp) {
			Ok(members) => {
				sets_by_index.insert(idx, members);
			}
			Err(err) => return err,
		}
	}

	let Some(mut result) = sets_by_index.remove(&0) else {
		return RespValue::Array(Vec::new());
	};
	for (_, set) in sets_by_index {
		result = result.difference(&set).cloned().collect();
	}
	bulk_array_from_set(result)
}

fn members_from_response(resp: RespValue) -> Result<HashSet<Bytes>, RespValue> {
	match resp {
		RespValue::Array(values) => {
			let mut members = HashSet::new();
			for value in values {
				match value {
					RespValue::BulkString(member) => {
						members.insert(member);
					}
					other => return Err(unexpected_aggregate_response(other)),
				}
			}
			Ok(members)
		}
		RespValue::Null => Ok(HashSet::new()),
		other => Err(unexpected_aggregate_response(other)),
	}
}

fn bulk_array_from_set(set: HashSet<Bytes>) -> RespValue {
	let mut members: Vec<_> = set.into_iter().collect();
	members.sort();
	RespValue::Array(members.into_iter().map(RespValue::BulkString).collect())
}

fn unexpected_aggregate_response(resp: RespValue) -> RespValue {
	RespValue::error(format!(
		"ERR unexpected response in multi-key aggregation: {:?}",
		resp
	))
}
