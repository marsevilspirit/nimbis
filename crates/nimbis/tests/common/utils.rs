use std::error::Error;
use std::net::TcpListener;

pub fn pick_free_port() -> Result<u16, Box<dyn Error + Send + Sync>> {
	let listener = TcpListener::bind("127.0.0.1:0")?;
	Ok(listener.local_addr()?.port())
}
