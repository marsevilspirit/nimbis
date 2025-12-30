use nimbis::config;
use nimbis::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	config::init_config();
	telemetry::init();

	let server = Server::new().await?;
	server.run().await?;

	Ok(())
}
