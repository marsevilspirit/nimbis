use config::Cli;
use config::Parser;
use nimbis::logo;
use nimbis::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let args = Cli::parse();

	config::setup(args);
	telemetry::logger::init();

	logo::show_logo();

	let server = Server::new().await?;
	server.run().await?;

	Ok(())
}
