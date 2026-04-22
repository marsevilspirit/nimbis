use std::error::Error;
use std::net::TcpListener;

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub fn pick_free_port() -> TestResult<u16> {
	let listener = TcpListener::bind("127.0.0.1:0")?;
	Ok(listener.local_addr()?.port())
}
