use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use bytes::Bytes;
use nimbis_resp::RespValue;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

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
	Local { request: ParsedCmd },
	Coordinated(CoordinatedCommandPlan),
}

#[derive(Debug, Clone)]
pub enum CoordinatedCommandPlan {
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
}

impl From<CoordinatedCommandPlan> for CommandPlan {
	fn from(plan: CoordinatedCommandPlan) -> Self {
		Self::Coordinated(plan)
	}
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

	pub fn execute(&mut self, plan: CoordinatedCommandPlan) {
		match plan {
			CoordinatedCommandPlan::SingleKey { key, request } => {
				let worker_idx = self.worker_for_key(&key);
				self.push_worker_request(worker_idx, request);
			}
			CoordinatedCommandPlan::Broadcast { request } => self.broadcast(request),
			CoordinatedCommandPlan::Scatter {
				subrequests,
				aggregate,
			} => self.scatter(subrequests, aggregate),
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
