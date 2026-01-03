use nimbis::config;
use nimbis::logo;
use nimbis::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	config::init_config();
	telemetry::init();

	println!("{}\tv{}", logo::LOGO.trim_end(), env!("CARGO_PKG_VERSION"));

	let server = Server::new().await?;
	server.run().await?;

	Ok(())
}
