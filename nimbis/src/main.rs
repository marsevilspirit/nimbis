use clap::Parser;
use nimbis::cli::Cli;
use nimbis::config::SERVER_CONF;
use nimbis::logo;
use nimbis::server::Server;
use nimbis_telemetry::manager::TELEMETRY_MANAGER;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let args = Cli::parse();

	if let Err(e) = nimbis::config::setup(args) {
		log::error!("Failed to load configuration: {}", e);
		std::process::exit(1);
	}

	logo::show_logo();

	let runtime_threads = SERVER_CONF.load().runtime_threads;
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.worker_threads(runtime_threads)
		.enable_all()
		.build()?;

	let result = runtime.block_on(async {
		let server = Server::new().await?;
		tokio::select! {
			result = server.run() => result,
			signal = tokio::signal::ctrl_c() => {
				signal?;
				log::info!("Shutdown signal received");
				Ok(())
			}
		}
	});

	TELEMETRY_MANAGER.load().flush();
	result?;

	Ok(())
}
