use std::collections::HashMap;
use std::sync::Arc;

use storage::Storage;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::error;
use tracing::info;

use crate::cmd::CmdTable;
use crate::config::SERVER_CONF;
use crate::worker::Worker;
use crate::worker::WorkerMessage;

pub struct Server {
	workers: Vec<Worker>,
}

impl Server {
	// Create a new server instance
	pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
		// Ensure data directory exists
		let data_path = &SERVER_CONF.load().data_path;
		std::fs::create_dir_all(data_path)?;
		let storage = Arc::new(Storage::open(data_path).await?);
		let cmd_table = Arc::new(CmdTable::new());

		let workers_num = num_cpus::get();
		let mut workers = Vec::with_capacity(workers_num);

		// First pass: create channels
		let mut senders = HashMap::with_capacity(workers_num);
		let mut receivers = Vec::with_capacity(workers_num);

		for i in 0..workers_num {
			let (tx, rx) = mpsc::unbounded_channel();
			senders.insert(i, tx);
			receivers.push(rx);
		}

		// Second pass: create workers
		for (i, rx) in receivers.into_iter().enumerate() {
			let my_tx = senders.get(&i).unwrap().clone();
			// workers need full map of senders to communicate with peers
			workers.push(Worker::new(
				i,
				my_tx,
				rx,
				senders.clone(),
				storage.clone(),
				cmd_table.clone(),
			));
		}

		Ok(Self { workers })
	}

	pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let addr = &SERVER_CONF.load().addr;
		let listener = TcpListener::bind(addr).await?;
		info!("Nimbis server listening on {}", addr);

		// We don't need Arc<Vec<Worker>> anymore because we are just dispatching messages
		let workers_len = self.workers.len();
		let mut next_worker_idx = 0;

		loop {
			match listener.accept().await {
				Ok((socket, addr)) => {
					info!("New client connected from {}", addr);

					// Round-robin dispatch
					let worker = &self.workers[next_worker_idx];
					if let Err(e) = worker.tx.send(WorkerMessage::NewConnection(socket)) {
						error!(
							"Failed to dispatch connection to worker {}: {}",
							next_worker_idx, e
						);
					}

					next_worker_idx = (next_worker_idx + 1) % workers_len;
				}
				Err(e) => {
					error!("Error accepting connection: {}", e);
				}
			}
		}
	}
}
