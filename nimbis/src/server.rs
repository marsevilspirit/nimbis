use std::sync::Arc;

use fastrace::trace;
use log::debug;
use log::error;
use log::info;
use nimbis_storage::Storage;
use tokio::net::TcpListener;

use crate::GCTX;
use crate::client::ClientConnection;
use crate::client::ClientSessions;
use crate::client::next_client_session_id;
use crate::cmd::CmdContext;
use crate::cmd::CmdTable;
use crate::context::init_global_context;
use crate::server_config;

pub struct Server {
	storage: Arc<Storage>,
	cmd_table: Arc<CmdTable>,
	_client_sessions: Arc<ClientSessions>,
}

impl Server {
	// Create a new server instance
	#[trace]
	pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
		let client_sessions = Arc::new(ClientSessions::new());
		init_global_context(client_sessions.clone());
		let cmd_table = Arc::new(CmdTable::new());

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
			cmd_table,
			_client_sessions: client_sessions,
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
					let cmd_table = self.cmd_table.clone();
					tokio::spawn(async move {
						let client_id = next_client_session_id();
						let ctx = CmdContext { client_id };
						let mut session = ClientConnection::new(socket, storage, cmd_table, ctx);
						GCTX!(client_sessions).register(client_id);
						if let Err(e) = session.run().await {
							debug!("Client session error: {}", e);
						}
						GCTX!(client_sessions).unregister(client_id);
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
