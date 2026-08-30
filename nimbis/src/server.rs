use std::sync::Arc;

use fastrace::trace;
use log::debug;
use log::error;
use log::info;
use nimbis_storage::Storage;
use tokio::net::TcpListener;

use crate::client::ClientConnection;
use crate::client::ClientSessions;
use crate::client::next_client_session_id;
use crate::cmd::CmdContext;
use crate::server_config;

pub struct Server {
	storage: Arc<Storage>,
	client_sessions: Arc<ClientSessions>,
}

impl Server {
	// Create a new server instance
	#[trace]
	pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
		let client_sessions = Arc::new(ClientSessions::new());

		let config = crate::config::SERVER_CONF.load();
		let object_store_url = config.object_store_url.clone();
		let object_store_options = config.object_store_options.0.clone();
		drop(config);

		let storage = Arc::new(
			Storage::open_object_store(
				&object_store_url,
				object_store_options
					.iter()
					.map(|(key, value)| (key.as_str(), value.as_str())),
				None,
			)
			.await?,
		);

		Ok(Self {
			storage,
			client_sessions,
		})
	}

	#[trace]
	pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let addr = format!("{}:{}", server_config!(host), server_config!(port));
		let listener = TcpListener::bind(&addr).await?;
		info!("Nimbis server listening on {}", addr);

		loop {
			debug!("Waiting for accept...");
			match listener.accept().await {
				Ok((socket, addr)) => {
					debug!("New client connected from {}", addr);

					let storage = self.storage.clone();
					let client_sessions = self.client_sessions.clone();
					tokio::spawn(async move {
						let client_id = next_client_session_id();
						let ctx = CmdContext {
							client_id,
							client_sessions: client_sessions.clone(),
						};
						let mut session = ClientConnection::new(socket, storage, ctx);
						client_sessions.register(client_id);
						if let Err(e) = session.run().await {
							debug!("Client session error: {}", e);
						}
						client_sessions.unregister(client_id);
					});
				}
				Err(e) => {
					error!("Error accepting connection: {}", e);
					tokio::time::sleep(std::time::Duration::from_millis(500)).await;
				}
			}
		}
	}
}
