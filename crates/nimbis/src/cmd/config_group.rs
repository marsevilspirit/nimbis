use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use resp::RespValue;
use storage::Storage;

use crate::cmd::Cmd;
use crate::cmd::CmdMeta;

/// SET command implementation
pub struct ConfigCommandGroup {
	meta: CmdMeta,
	sub_cmds: HashMap<String, Box<dyn Cmd>>,
}

impl ConfigCommandGroup {
	pub fn new() -> Self {
		let mut sub_cmds: HashMap<String, Box<dyn Cmd>> = HashMap::new();

		sub_cmds.insert("GET".to_string(), Box::new(ConfigGetCommand::new()));

		Self {
			meta: CmdMeta {
				name: "CONFIG".to_string(),
				arity: -3,
			},
			sub_cmds,
		}
	}
}

#[async_trait]
impl Cmd for ConfigCommandGroup {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue {
		// First argument should be the subcommand name (e.g., "GET")
		if args.is_empty() {
			return RespValue::error("ERR wrong number of arguments for CONFIG command");
		}

		// Convert first argument to uppercase for case-insensitive lookup
		// TODO: find a better way to do this
		let sub_cmd_name = String::from_utf8_lossy(&args[0]).to_uppercase();

		// Find and execute the subcommand
		match self.sub_cmds.get(&sub_cmd_name) {
			Some(sub_cmd) => {
				// Pass remaining arguments to the subcommand
				sub_cmd.execute(storage, &args[1..]).await
			}
			None => RespValue::error(format!("ERR unknown CONFIG subcommand '{}'", sub_cmd_name)),
		}
	}
}

pub struct ConfigGetCommand {
	meta: CmdMeta,
}

impl ConfigGetCommand {
	pub fn new() -> Self {
		Self {
			meta: CmdMeta {
				name: "GET".to_string(),
				arity: 2,
			},
		}
	}
}

#[async_trait]
impl Cmd for ConfigGetCommand {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Arc<Storage>, _args: &[bytes::Bytes]) -> RespValue {
		// TODO: Implement CONFIG GET command logic
		RespValue::error("CONFIG GET not implemented yet")
	}
}
