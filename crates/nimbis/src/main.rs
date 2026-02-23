use nimbis::config::Cli;
use nimbis::config::Parser;
use nimbis::logo;
use nimbis::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let args = Cli::parse();

	if let Err(e) = nimbis::config::setup(args) {
		log::error!("Failed to load configuration: {}", e);
		std::process::exit(1);
	}

	logo::show_logo();

	let server = Server::new().await?;
	server.run().await?;

	Ok(())
}
