#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::Child;
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::process::ExitStatus;
#[cfg(unix)]
use std::process::Stdio;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

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
			block_cache_capacity_bytes: nimbis_storage::DEFAULT_BLOCK_CACHE_CAPACITY_BYTES,
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
			runtime_threads: 2,
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
	let deadline = Instant::now() + ready_timeout;
	let mut last_error = String::from("server was not probed");

	while Instant::now() < deadline {
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
		thread::sleep(Duration::from_millis(100));
	}

	panic!(
		"nimbis did not become ready at {}:{} within {:?}; object_store_url={}; last_error={}",
		host, port, ready_timeout, object_store_url, last_error
	);
}

#[cfg(unix)]
pub struct MockNimbisProcess(Child);

#[cfg(unix)]
impl MockNimbisProcess {
	pub fn start(directory: &Path, port: u16) -> (Self, MockNimbisClient) {
		let config_path = directory.join("recovery.toml");
		let object_store_url = url::Url::from_directory_path(directory.join("store"))
			.unwrap()
			.to_string();
		fs::write(
			&config_path,
			format!(
				"host = '127.0.0.1'\nport = {port}\nobject_store_url = '{object_store_url}'\nlog_level = 'error,nimbis::server=info'\nruntime_threads = 2\nblock_cache_capacity_bytes = 8388608\n"
			),
		)
		.unwrap();
		let log_path = directory.join("recovery-server.log");
		let log = File::create(&log_path).unwrap();
		let mut process = Self(
			Command::new(env!("CARGO_BIN_EXE_nimbis"))
				.env_clear()
				.env("NIMBIS_TRACE_ENABLED", "false")
				.arg("--config")
				.arg(config_path)
				.stdout(Stdio::from(log.try_clone().unwrap()))
				.stderr(Stdio::from(log))
				.spawn()
				.unwrap(),
		);
		let deadline = Instant::now() + Duration::from_secs(15);
		let marker = format!("Nimbis server listening on 127.0.0.1:{port}");
		loop {
			let log = fs::read_to_string(&log_path).unwrap();
			assert!(
				process.0.try_wait().unwrap().is_none(),
				"server exited before readiness: {log}"
			);
			if log.contains(&marker)
				&& let Ok(mut client) = MockNimbisClient::connect("127.0.0.1", port)
			{
				assert_eq!(client.ping(), "PONG");
				assert!(
					process.0.try_wait().unwrap().is_none(),
					"server exited after PING"
				);
				return (process, client);
			}
			assert!(Instant::now() < deadline, "server startup hung: {log}");
			thread::sleep(Duration::from_millis(10));
		}
	}

	pub fn stop(&mut self, signal: &str) -> ExitStatus {
		assert!(
			Command::new("kill")
				.args([signal, &self.0.id().to_string()])
				.status()
				.unwrap()
				.success()
		);
		let deadline = Instant::now() + Duration::from_secs(15);
		loop {
			if let Some(status) = self.0.try_wait().unwrap() {
				return status;
			}
			assert!(Instant::now() < deadline, "server shutdown hung");
			thread::sleep(Duration::from_millis(10));
		}
	}
}

#[cfg(unix)]
impl Drop for MockNimbisProcess {
	fn drop(&mut self) {
		let _ = self.0.kill();
		let _ = self.0.wait();
	}
}
