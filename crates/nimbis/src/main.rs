use nimbis::config;
use nimbis::logo;
use nimbis::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	config::init_config();
	telemetry::logger::init();

	logo::show_logo();

	let server = Server::new().await?;
	server.run().await?;

	Ok(())
}
