use std::future::Future;
use std::process;
use std::sync::Arc;

use fastrace::trace;
use log::debug;
use log::error;
use log::info;
use nimbis_storage::Storage;
use nimbis_storage::StorageError;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::signal::unix::SignalKind;
#[cfg(unix)]
use tokio::signal::unix::signal;
use tokio::task::JoinSet;

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
		let block_cache_capacity_bytes = config.block_cache_capacity_bytes;
		drop(config);

		let storage = Arc::new(
			Storage::open_object_store(
				&object_store_url,
				object_store_options
					.iter()
					.map(|(key, value)| (key.as_str(), value.as_str())),
				None,
				block_cache_capacity_bytes,
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
		let mut sessions = JoinSet::new();
		let shutdown = shutdown_signal();
		tokio::pin!(shutdown);

		let result = loop {
			debug!("Waiting for accept...");
			let accepted = tokio::select! {
				biased;
				result = &mut shutdown => break result,
				Some(result) = sessions.join_next() => {
					if let Err(error) = result {
						error!("Client task failed: {}", error);
					}
					continue;
				}
				accepted = listener.accept() => accepted,
			};
			match accepted {
				Ok((socket, addr)) => {
					debug!("New client connected from {}", addr);

					let storage = self.storage.clone();
					let cmd_table = self.cmd_table.clone();
					sessions.spawn(async move {
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
		};
		drop(listener);
		info!("Shutdown requested; send another shutdown signal to force exit");
		finish_shutdown(async {
			// Stop every command producer before flushing storage. Requests without a
			// response may have executed; callers must treat their outcome as unknown.
			sessions.abort_all();
			while let Some(result) = sessions.join_next().await {
				if let Err(error) = result
					&& !error.is_cancelled()
				{
					error!("Client task failed during shutdown: {}", error);
				}
			}
			self.storage.close().await
		})
		.await?;
		result?;
		info!("Storage closed; shutdown complete");
		Ok(())
	}
}

async fn finish_shutdown(
	close: impl Future<Output = Result<(), StorageError>>,
) -> Result<(), StorageError> {
	tokio::select! {
		biased;
		signal = shutdown_signal() => {
			signal?;
			error!("Second shutdown signal received; forcing exit before storage close completes");
			// Returning an error can still block in Runtime::drop on stuck tasks.
			process::exit(1);
		}
		result = close => result,
	}
}

async fn shutdown_signal() -> std::io::Result<()> {
	#[cfg(unix)]
	{
		let mut terminate = signal(SignalKind::terminate())?;
		tokio::select! {
			result = tokio::signal::ctrl_c() => result,
			_ = terminate.recv() => Ok(()),
		}
	}
	#[cfg(not(unix))]
	tokio::signal::ctrl_c().await
}

#[cfg(all(test, unix))]
mod tests {
	use std::env;
	use std::future::pending;
	use std::io;
	use std::io::Write;
	use std::process::Stdio;
	use std::time::Duration;

	use rstest::rstest;
	use tokio::io::AsyncBufReadExt;
	use tokio::io::BufReader;
	use tokio::process::Command;
	use tokio::time::timeout;

	use super::*;

	#[rstest]
	#[case("-INT")]
	#[case("-TERM")]
	#[tokio::test]
	async fn second_signal_exits_during_pending_close(#[case] signal: &str) {
		let mut child = Command::new(env::current_exe().unwrap())
			.args([
				"--exact",
				"server::tests::shutdown_signal_child",
				"--nocapture",
			])
			.env("NIMBIS_SHUTDOWN_TEST_CHILD", "1")
			.stdout(Stdio::piped())
			.kill_on_drop(true)
			.spawn()
			.unwrap();
		let pid = child.id().unwrap().to_string();
		let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
		for marker in ["READY", "CLOSING"] {
			timeout(Duration::from_secs(10), async {
				loop {
					let line = lines
						.next_line()
						.await
						.unwrap()
						.expect("child exited early");
					if line == marker {
						break;
					}
				}
			})
			.await
			.unwrap();
			assert!(
				Command::new("kill")
					.args([signal, &pid])
					.status()
					.await
					.unwrap()
					.success()
			);
		}
		let status = timeout(Duration::from_secs(10), child.wait())
			.await
			.unwrap()
			.unwrap();
		assert_eq!(status.code(), Some(1));
	}

	#[tokio::test]
	async fn shutdown_signal_child() {
		if env::var_os("NIMBIS_SHUTDOWN_TEST_CHILD").is_none() {
			return;
		}
		tokio::select! {
			biased;
			result = shutdown_signal() => result.unwrap(),
			_ = async {
				io::stdout().write_all(b"READY\n").unwrap();
				io::stdout().flush().unwrap();
				pending::<()>().await;
			} => unreachable!(),
		}
		finish_shutdown(async {
			// This is polled only after the escalation signal handler is listening.
			io::stdout().write_all(b"CLOSING\n").unwrap();
			io::stdout().flush().unwrap();
			pending().await
		})
		.await
		.unwrap();
		panic!("pending close unexpectedly completed");
	}
}
