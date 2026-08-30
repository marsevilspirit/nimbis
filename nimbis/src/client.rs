use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use bytes::Bytes;
use bytes::BytesMut;
use fastrace::future::FutureExt;
use fastrace::prelude::Span;
use fastrace::prelude::SpanContext;
use fastrace::trace;
use log::debug;
use nimbis_resp::RespEncoder;
use nimbis_resp::RespParseResult;
use nimbis_resp::RespParser;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::cmd::CmdContext;
use crate::cmd::ParsedCmd;
use crate::cmd::execute;
use crate::server_config;

static NEXT_CLIENT_SESSION_ID: AtomicI64 = AtomicI64::new(1);

pub fn next_client_session_id() -> i64 {
	NEXT_CLIENT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Default)]
pub struct ClientSessions {
	sessions: Mutex<BTreeMap<i64, Option<Bytes>>>,
}

impl ClientSessions {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn register(&self, client_id: i64) {
		self.lock().entry(client_id).or_default();
	}

	pub fn unregister(&self, client_id: i64) {
		self.lock().remove(&client_id);
	}

	pub fn set_name(&self, client_id: i64, name: Bytes) -> bool {
		let mut sessions = self.lock();
		let Some(current_name) = sessions.get_mut(&client_id) else {
			return false;
		};
		*current_name = Some(name);
		true
	}

	pub fn get_name(&self, client_id: i64) -> Option<Bytes> {
		self.lock().get(&client_id).cloned().flatten()
	}

	pub fn list(&self) -> Vec<(i64, Option<Bytes>)> {
		self.lock()
			.iter()
			.map(|(&client_id, name)| (client_id, name.clone()))
			.collect()
	}

	fn lock(&self) -> MutexGuard<'_, BTreeMap<i64, Option<Bytes>>> {
		self.sessions
			.lock()
			.unwrap_or_else(|poisoned| poisoned.into_inner())
	}
}

pub struct ClientConnection {
	socket: TcpStream,
	parser: RespParser,
	storage: Arc<Storage>,
	ctx: CmdContext,
}

impl ClientConnection {
	pub fn new(socket: TcpStream, storage: Arc<Storage>, ctx: CmdContext) -> Self {
		Self {
			socket,
			parser: RespParser::new(),
			storage,
			ctx,
		}
	}

	#[trace]
	pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
		let mut buffer = BytesMut::with_capacity(4096);
		debug!("Client connection started");

		loop {
			let n = match self.socket.read_buf(&mut buffer).await {
				Ok(n) => n,
				Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
					debug!("Connection reset by peer");
					return Ok(());
				}
				Err(e) => return Err(e.into()),
			};
			debug!("Read {} bytes from socket", n);

			if n == 0 {
				if buffer.is_empty() {
					return Ok(());
				} else {
					return Err("Connection closed with incomplete data".into());
				}
			}

			let mut parsed_cmds = Vec::new();

			loop {
				match self.parser.parse(&mut buffer) {
					RespParseResult::Complete(value) => {
						let parsed_cmd: ParsedCmd = match value.try_into() {
							Ok(cmd) => cmd,
							Err(e) => {
								let error_response =
									RespValue::error(format!("ERR Protocol error: {}", e));
								self.socket.write_all(&error_response.encode()?).await?;
								return Err(e.into());
							}
						};
						parsed_cmds.push(parsed_cmd);
					}
					RespParseResult::Incomplete => {
						break;
					}
					RespParseResult::Error(e) => {
						let error_response = RespValue::error(format!("ERR Protocol error: {}", e));
						match self.socket.write_all(&error_response.encode()?).await {
							Err(e) if e.kind() != std::io::ErrorKind::ConnectionReset => {
								return Err(e.into());
							}
							_ => {}
						}
						return Err(e.into());
					}
				}
			}

			for parsed_cmd in parsed_cmds {
				let response = self.execute_command(parsed_cmd).await;
				if let Err(e) = self.socket.write_all(&response.encode()?).await {
					if e.kind() == std::io::ErrorKind::ConnectionReset {
						debug!("Connection reset by peer");
						return Ok(());
					}
					return Err(e.into());
				}
			}
		}
	}

	async fn execute_command(&self, parsed_cmd: ParsedCmd) -> RespValue {
		if !server_config!(trace_enabled) {
			return self.execute_command_inner(parsed_cmd).await;
		}

		let sampling_ratio = server_config!(trace_sampling_ratio);
		let is_sampled = should_sample(sampling_ratio);
		let span_context = SpanContext::random().sampled(is_sampled);
		let root_span = Span::root(fastrace::func_path!(), span_context).with_properties(|| {
			[
				("cmd", parsed_cmd.name().to_uppercase()),
				("client_id", self.ctx.client_id.to_string()),
			]
		});

		self.execute_command_inner(parsed_cmd)
			.in_span(root_span)
			.await
	}

	#[trace]
	async fn execute_command_inner(&self, parsed_cmd: ParsedCmd) -> RespValue {
		execute(
			parsed_cmd.name(),
			&self.storage,
			&parsed_cmd.args,
			&self.ctx,
		)
		.await
		.unwrap_or_else(|| {
			RespValue::error(format!(
				"ERR unknown command '{}'",
				parsed_cmd.name().to_lowercase()
			))
		})
	}
}

fn should_sample(sampling_ratio: f64) -> bool {
	if sampling_ratio <= 0.0 {
		return false;
	}

	if sampling_ratio >= 1.0 {
		return true;
	}

	rand::random::<f64>() < sampling_ratio
}

#[cfg(test)]
mod tests {
	use super::should_sample;

	#[test]
	fn test_should_sample_limits() {
		assert!(should_sample(1.0));
		assert!(!should_sample(0.0));
	}
}
