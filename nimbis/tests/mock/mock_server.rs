use std::sync::Arc;
use std::time::Duration;

use nimbis::config::SERVER_CONF;
use nimbis::config::ServerConfig;
use nimbis::server::Server;
use nimbis_telemetry::manager::TELEMETRY_MANAGER;
use nimbis_telemetry::manager::TelemetryManager;
use tempfile::TempDir;
use tempfile::tempdir;
use tokio::runtime::Builder;
use tokio::runtime::Runtime;

use crate::mock::mock_client::MockNimbisClient;
use crate::mock::utils::pick_free_port;

pub struct MockNimbisServer {
	host: String,
	port: u16,
	_data_dir: TempDir,
	runtime: Option<Runtime>,
}

impl MockNimbisServer {
	pub fn new() -> Self {
		let port = pick_free_port().expect("pick free port");
		let data_dir = tempdir().expect("create temp dir");
		let object_store_url = url::Url::from_directory_path(data_dir.path())
			.expect("convert temp dir path to file URL")
			.to_string();

		let config = ServerConfig {
			host: "127.0.0.1".to_string(),
			port,
			object_store_url: object_store_url.clone(),
			object_store_options: Default::default(),
			save: "".to_string(),
			appendonly: "no".to_string(),
			log_level: "error".to_string(),
			log_output: "terminal".to_string(),
			log_rotation: "daily".to_string(),
			trace_enabled: false,
			trace_endpoint: "".to_string(),
			trace_sampling_ratio: 0.0001,
			trace_protocol: "grpc".to_string(),
			trace_export_timeout_seconds: 10,
			trace_report_interval_ms: 1000,
			worker_threads: 2,
		};

		SERVER_CONF.init(config.clone());
		SERVER_CONF.update(config);

		let runtime = Builder::new_multi_thread()
			.enable_all()
			.build()
			.expect("build tokio runtime");
		TELEMETRY_MANAGER.init(Arc::new(TelemetryManager::disabled()));
		runtime.spawn(async move {
			match Server::new().await {
				Ok(server) => {
					if let Err(e) = server.run().await {
						log::error!("mock nimbis server exited: {}", e);
					}
				}
				Err(e) => {
					log::error!("mock nimbis server failed to start: {}", e);
				}
			}
		});

		wait_until_ready("127.0.0.1", port, &object_store_url);

		Self {
			host: "127.0.0.1".to_string(),
			port,
			_data_dir: data_dir,
			runtime: Some(runtime),
		}
	}

	pub fn get_client(&self) -> MockNimbisClient {
		MockNimbisClient::connect(&self.host, self.port).expect("connect to nimbis")
	}
}

impl Drop for MockNimbisServer {
	fn drop(&mut self) {
		if let Some(runtime) = self.runtime.take() {
			runtime.shutdown_timeout(Duration::from_secs(1));
		}
	}
}

fn wait_until_ready(host: &str, port: u16, object_store_url: &str) {
	// Keep startup failures quick while still allowing slower CI hosts
	// enough time to initialize SlateDB.
	let ready_timeout = Duration::from_secs(15);
	let deadline = std::time::Instant::now() + ready_timeout;
	let mut last_error = String::from("server was not probed");

	while std::time::Instant::now() < deadline {
		match MockNimbisClient::connect(host, port).map(|mut client| client.ping()) {
			Ok(resp) if resp == "PONG" => return,
			Ok(resp) => {
				last_error = format!("unexpected ready response: {}", resp);
			}
			Err(e) => {
				last_error = e.to_string();
			}
		}

		// Poll often enough to keep the test suite snappy.
		std::thread::sleep(Duration::from_millis(100));
	}

	panic!(
		"nimbis did not become ready at {}:{} within {:?}; object_store_url={}; last_error={}",
		host, port, ready_timeout, object_store_url, last_error
	);
}
