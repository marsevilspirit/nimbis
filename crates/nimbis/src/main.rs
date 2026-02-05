use nimbis::config::Cli;
use nimbis::config::Parser;
use nimbis::logo;
use nimbis::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let args = Cli::parse();

	telemetry::logger::init(&args.log_level);
	nimbis::config::setup(args);

	logo::show_logo();

	let server = Server::new().await?;
	server.run().await?;

	Ok(())
}
