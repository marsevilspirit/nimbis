use nimbis::server::Server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	telemetry::init();

	let server = Server::new("127.0.0.1:6379").await?;
	server.run().await?;

	Ok(())
}
