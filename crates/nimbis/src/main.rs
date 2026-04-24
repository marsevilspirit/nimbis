use clap::Parser;
use nimbis::cli::Cli;
use nimbis::logo;
use nimbis::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let args = Cli::parse();

	let telemetry_manager = match nimbis::config::setup(args) {
		Ok(manager) => manager,
		Err(e) => {
			log::error!("Failed to load configuration: {}", e);
			std::process::exit(1);
		}
	};

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

	telemetry_manager.flush();
	result?;

	Ok(())
}
