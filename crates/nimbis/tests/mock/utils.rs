use std::error::Error;
use std::net::TcpListener;

use resp::RespValue;

pub fn pick_free_port() -> Result<u16, Box<dyn Error + Send + Sync>> {
	let listener = TcpListener::bind("127.0.0.1:0")?;
	Ok(listener.local_addr()?.port())
}

pub fn resp_error(resp: RespValue) -> String {
	match resp {
		RespValue::Error(e) | RespValue::BulkError(e) => String::from_utf8_lossy(&e).into_owned(),
		other => panic!("expected error response, got: {:?}", other),
	}
}

pub fn resp_to_strings(resp: RespValue) -> Vec<String> {
	match resp {
		RespValue::Array(arr) => arr
			.into_iter()
			.map(|v| match v {
				RespValue::Null => String::new(),
				other => other.to_string_lossy().unwrap_or_default(),
			})
			.collect(),
		RespValue::Null => vec![],
		_ => panic!("expected array response, got: {:?}", resp),
	}
}
