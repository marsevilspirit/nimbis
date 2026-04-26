pub mod benchmarks;
pub mod checks;

use std::io::Write;

pub fn write_stdout(value: &str) -> Result<(), String> {
	std::io::stdout()
		.write_all(value.as_bytes())
		.map_err(|error| error.to_string())
}

pub fn write_stdout_line(value: &str) -> Result<(), String> {
	write_stdout(&format!("{value}\n"))
}

pub fn write_stderr_line(value: &str) {
	let _ = std::io::stderr().write_all(format!("{value}\n").as_bytes());
}
