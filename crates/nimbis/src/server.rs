use std::collections::HashMap;
use std::sync::Arc;

use log::debug;
use log::error;
use log::info;
use storage::Storage;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::cmd::CmdTable;
use crate::server_config;
use crate::worker::Worker;
use crate::worker::WorkerMessage;

pub struct Server {
	workers: Vec<Worker>,
}

impl Server {
	// Create a new server instance
	pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
		// Ensure data directory exists
		let data_path = server_config!(data_path);
		std::fs::create_dir_all(data_path)?;

		let cmd_table = Arc::new(CmdTable::new());

		// Determine number of workers based on CPU cores
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

		// Wrap senders in Arc to avoid deep cloning for each worker
		let senders = Arc::new(senders);

		// Second pass: create workers and sharded storage
		for (i, rx) in receivers.into_iter().enumerate() {
			let my_tx = senders.get(&i).unwrap().clone();

			// SHARDED STORAGE: Create a unique Storage instance for this worker
			// Data will be in .../nimbis_data/shard-{i}/...
			let storage = Arc::new(Storage::open(data_path, Some(i)).await?);

			// workers need the full map of senders to route commands to the appropriate
			// worker based on consistent hashing of the command's key
			workers.push(Worker::new(
				i,
				my_tx,
				rx,
				senders.clone(),
				storage, // This is now unique per worker
				cmd_table.clone(),
			));
		}

		Ok(Self { workers })
	}

	pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let addr = server_config!(addr);
		let listener = TcpListener::bind(addr).await?;
		info!("Nimbis server listening on {}", addr);

		let workers_len = self.workers.len();
		let mut next_worker_idx = 0;

		loop {
			debug!("Waiting for accept...");
			match listener.accept().await {
				Ok((socket, addr)) => {
					debug!("New client connected from {}", addr);

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
					tokio::time::sleep(std::time::Duration::from_millis(500)).await;
				}
			}
		}
	}
}
