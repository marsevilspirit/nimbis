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

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue;

	/// Execute the command
	async fn execute(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue {
		if let Err(err) = self.meta().validate_arity(args.len() + 1) {
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

mod cmd_del;
mod cmd_exists;
mod cmd_expire;
mod cmd_get;
mod cmd_hget;
mod cmd_hgetall;
mod cmd_hlen;
mod cmd_hmget;
mod cmd_hset;
mod cmd_llen;
mod cmd_lpop;
mod cmd_lpush;
mod cmd_lrange;
mod cmd_ping;
mod cmd_rpop;
mod cmd_rpush;
mod cmd_sadd;
mod cmd_scard;
mod cmd_set;
mod cmd_sismember;
mod cmd_smembers;
mod cmd_srem;
mod cmd_ttl;
mod group_cmd_config;
mod table;
pub mod utils;

pub use cmd_del::DelCmd;
pub use cmd_exists::ExistsCmd;
pub use cmd_expire::ExpireCmd;
pub use cmd_get::GetCmd;
pub use cmd_hget::HGetCmd;
pub use cmd_hgetall::HGetAllCmd;
pub use cmd_hlen::HLenCmd;
pub use cmd_hmget::HMGetCmd;
pub use cmd_hset::HSetCmd;
pub use cmd_llen::LLenCmd;
pub use cmd_lpop::LPopCmd;
pub use cmd_lpush::LPushCmd;
pub use cmd_lrange::LRangeCmd;
pub use cmd_ping::PingCmd;
pub use cmd_rpop::RPopCmd;
pub use cmd_rpush::RPushCmd;
pub use cmd_sadd::SaddCmd;
pub use cmd_scard::ScardCmd;
pub use cmd_set::SetCmd;
pub use cmd_sismember::SismemberCmd;
pub use cmd_smembers::SmembersCmd;
pub use cmd_srem::SremCmd;
pub use cmd_ttl::TtlCmd;
pub use group_cmd_config::ConfigGroupCmd;
pub use table::CmdTable;

mod cmd_zadd;
pub use cmd_zadd::ZAddCmd;
mod cmd_zrange;
pub use cmd_zrange::ZRangeCmd;
mod cmd_zscore;
pub use cmd_zscore::ZScoreCmd;
mod cmd_zrem;
pub use cmd_zrem::ZRemCmd;
mod cmd_zcard;
pub use cmd_zcard::ZCardCmd;
