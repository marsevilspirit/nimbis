use clap::Parser;
use nimbis::cli::Cli;
use nimbis::logo;
use nimbis::server::Server;
use telemetry::manager::TELEMETRY_MANAGER;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let args = Cli::parse();

	if let Err(e) = nimbis::config::setup(args) {
		log::error!("Failed to load configuration: {}", e);
		std::process::exit(1);
	}

	logo::show_logo();

	let result = async {
		let server = Server::new().await?;
		tokio::select! {
			result = server.run() => result,
			signal = tokio::signal::ctrl_c() => {
				signal?;
				log::info!("Shutdown signal received");
				Ok(())
			}
		}
	}
	.await;

	TELEMETRY_MANAGER.load().flush();
	result?;

	Ok(())
}
