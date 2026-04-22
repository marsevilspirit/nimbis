use std::collections::HashMap;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use crate::config::SERVER_CONF;
use crate::config::ServerConfig;

/// Config command implementation
pub struct ConfigCmd {
	meta: CmdMeta,
	sub_cmds: HashMap<&'static str, Box<dyn Cmd>>,
}

impl Default for ConfigCmd {
	fn default() -> Self {
		let mut sub_cmds: HashMap<&'static str, Box<dyn Cmd>> = HashMap::new();

		sub_cmds.insert("GET", Box::new(ConfigGetCmd::default()));
		sub_cmds.insert("SET", Box::new(ConfigSetCmd::default()));

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
impl Cmd for ConfigCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], ctx: &CmdContext) -> RespValue {
		// First argument should be the subcommand name (e.g., "GET")
		if args.is_empty() {
			return RespValue::error("ERR wrong number of arguments for CONFIG command");
		}

		// Convert first argument to uppercase for case-insensitive lookup
		// TODO: find a better way to do this
		let sub_cmd_name = String::from_utf8_lossy(&args[0]).to_uppercase();

		// Find and execute the subcommand
		match self.sub_cmds.get(sub_cmd_name.as_str()) {
			Some(sub_cmd) => {
				// Pass remaining arguments to the subcommand
				sub_cmd.execute(storage, &args[1..], ctx).await
			}
			None => RespValue::error(format!("ERR unknown CONFIG subcommand '{}'", sub_cmd_name)),
		}
	}
}

pub struct ConfigGetCmd {
	meta: CmdMeta,
}

impl Default for ConfigGetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "GET".to_string(),
				arity: 2,
			},
		}
	}
}

#[async_trait]
impl Cmd for ConfigGetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let pattern = String::from_utf8_lossy(&args[0]);

		// Check if pattern contains wildcard
		if pattern.contains('*') {
			// Get matching field names
			let matched_fields = ServerConfig::match_fields(&pattern);

			if matched_fields.is_empty() {
				// No fields matched
				return RespValue::array(vec![]);
			}

			// Get current config
			let config = SERVER_CONF.load();

			// Build result array: [key1, value1, key2, value2, ...]
			let mut result = Vec::new();
			for field_name in matched_fields {
				if let Ok(value) = config.get_field(field_name) {
					result.push(RespValue::bulk_string(Bytes::from(field_name.to_string())));
					result.push(RespValue::bulk_string(Bytes::from(value)));
				}
			}

			RespValue::array(result)
		} else {
			// Exact match - original behavior
			match SERVER_CONF.load().get_field(&pattern) {
				Ok(value) => {
					// CONFIG GET returns an array: [field_name, field_value]
					RespValue::array(vec![
						RespValue::bulk_string(Bytes::from(pattern.into_owned())),
						RespValue::bulk_string(Bytes::from(value)),
					])
				}
				Err(e) => RespValue::error(e),
			}
		}
	}
}

pub struct ConfigSetCmd {
	meta: CmdMeta,
}

impl Default for ConfigSetCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SET".to_string(),
				arity: 3, // CONFIG SET key value
			},
		}
	}
}

#[async_trait]
impl Cmd for ConfigSetCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let field_name = String::from_utf8_lossy(&args[0]);
		let value = String::from_utf8_lossy(&args[1]);

		// Load current config, clone it, and modify
		let current = SERVER_CONF.load();
		let mut new_config = (**current).clone();

		// Try to set the field
		match new_config.set_field(&field_name, &value) {
			Ok(_) => {
				// Update to the new config
				SERVER_CONF.update(new_config);
				RespValue::simple_string("OK")
			}
			Err(e) => RespValue::error(e),
		}
	}
}
