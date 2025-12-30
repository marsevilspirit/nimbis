use std::sync::Arc;

use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use super::cmd_meta::CmdMeta;

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
