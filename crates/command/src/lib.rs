use std::sync::Arc;

use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

/// Command metadata containing immutable information about a command
#[derive(Debug, Clone, Default)]
pub struct CmdMeta {
	pub name: String,
	pub arity: i16,
}

impl CmdMeta {
	/// Validate argument count against arity
	/// - Positive arity: requires exact match
	/// - Negative arity: allows up to abs(arity) arguments
	pub fn validate_arity(&self, arg_count: usize) -> Result<(), String> {
		if self.arity > 0 {
			// Positive: exact match required
			if arg_count != self.arity as usize {
				return Err(format!(
					"ERR wrong number of arguments for '{}' command",
					self.name.to_lowercase()
				));
			}
		} else if self.arity < 0 {
			// Negative: minimum count allowed
			let min_args = (-self.arity) as usize;
			if arg_count < min_args {
				return Err(format!(
					"ERR wrong number of arguments for '{}' command",
					self.name.to_lowercase()
				));
			}
		}
		// arity == 0 means any number of arguments is allowed
		Ok(())
	}
}

/// Command trait - all commands must implement this
#[async_trait]
pub trait Cmd: Send + Sync {
	/// Get command metadata
	fn meta(&self) -> &CmdMeta;

	fn validate_arity(&self, arg_count: usize) -> Result<(), String> {
		self.meta().validate_arity(arg_count)
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue;

	/// Execute the command
	async fn execute(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue {
		if let Err(err) = self.validate_arity(args.len() + 1) {
			return RespValue::error(err);
		}

		self.do_cmd(storage, args).await
	}
}

/// Parsed command structure (renamed from Cmd to avoid conflict)
pub struct ParsedCmd {
	pub name: String,
	pub args: Vec<bytes::Bytes>,
}

impl TryFrom<RespValue> for ParsedCmd {
	type Error = String;

	fn try_from(value: RespValue) -> Result<Self, Self::Error> {
		// RespValue should be an array
		let args = value.as_array().ok_or("Expected array")?;

		if args.is_empty() {
			return Err("Empty command".to_string());
		}

		// First element is the command name
		let cmd_name = args[0]
			.as_str()
			.ok_or("Invalid command type")?
			.to_uppercase();

		// Remaining elements are arguments
		let cmd_args: Result<Vec<bytes::Bytes>, _> = args[1..]
			.iter()
			.map(|v| v.as_bytes().cloned().ok_or("Invalid argument"))
			.collect();

		Ok(ParsedCmd {
			name: cmd_name,
			args: cmd_args?,
		})
	}
}

mod cmd_table;
mod get;
mod group_config;
mod ping;
mod set;

pub use cmd_table::CmdTable;
pub use get::GetCommand;
pub use group_config::ConfigCommandGroup;
pub use ping::PingCommand;
pub use set::SetCommand;
