use std::collections::HashMap;

use async_trait::async_trait;
use bytes::Bytes;
use resp::RespValue;
use storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use crate::client;
use crate::client::ClientSessions;

/// Client group command implementation.
pub struct ClientGroupCmd {
	meta: CmdMeta,
	sub_cmds: HashMap<String, Box<dyn Cmd>>,
}

impl ClientGroupCmd {
	pub fn new(client_sessions: ClientSessions) -> Self {
		let mut sub_cmds: HashMap<String, Box<dyn Cmd>> = HashMap::new();

		sub_cmds.insert("ID".to_string(), Box::new(ClientIdCommand::default()));
		sub_cmds.insert(
			"SETNAME".to_string(),
			Box::new(ClientSetNameCommand::new(client_sessions.clone())),
		);
		sub_cmds.insert(
			"GETNAME".to_string(),
			Box::new(ClientGetNameCommand::new(client_sessions.clone())),
		);
		sub_cmds.insert(
			"LIST".to_string(),
			Box::new(ClientListCommand::new(client_sessions)),
		);

		Self {
			meta: CmdMeta {
				name: "CLIENT".to_string(),
				arity: -2,
			},
			sub_cmds,
		}
	}
}

#[async_trait]
impl Cmd for ClientGroupCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], ctx: &CmdContext) -> RespValue {
		if args.is_empty() {
			return RespValue::error("ERR wrong number of arguments for CLIENT command");
		}

		let sub_cmd_name = String::from_utf8_lossy(&args[0]).to_uppercase();
		match self.sub_cmds.get(&sub_cmd_name) {
			Some(sub_cmd) => sub_cmd.execute(storage, &args[1..], ctx).await,
			None => RespValue::error(format!(
				"ERR unknown CLIENT subcommand '{}'",
				sub_cmd_name
			)),
		}
	}
}

pub struct ClientIdCommand {
	meta: CmdMeta,
}

impl Default for ClientIdCommand {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "ID".to_string(),
				arity: 1,
			},
		}
	}
}

#[async_trait]
impl Cmd for ClientIdCommand {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, _args: &[Bytes], ctx: &CmdContext) -> RespValue {
		RespValue::integer(ctx.client_id)
	}
}

pub struct ClientSetNameCommand {
	meta: CmdMeta,
	client_sessions: ClientSessions,
}

impl ClientSetNameCommand {
	fn new(client_sessions: ClientSessions) -> Self {
		Self {
			meta: CmdMeta {
				name: "SETNAME".to_string(),
				arity: 2,
			},
			client_sessions,
		}
	}
}

#[async_trait]
impl Cmd for ClientSetNameCommand {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, args: &[Bytes], ctx: &CmdContext) -> RespValue {
		if client::set_client_name(&self.client_sessions, ctx.client_id, args[0].clone()) {
			RespValue::simple_string("OK")
		} else {
			RespValue::error("ERR client not found")
		}
	}
}

pub struct ClientGetNameCommand {
	meta: CmdMeta,
	client_sessions: ClientSessions,
}

impl ClientGetNameCommand {
	fn new(client_sessions: ClientSessions) -> Self {
		Self {
			meta: CmdMeta {
				name: "GETNAME".to_string(),
				arity: 1,
			},
			client_sessions,
		}
	}
}

#[async_trait]
impl Cmd for ClientGetNameCommand {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, _args: &[Bytes], ctx: &CmdContext) -> RespValue {
		match client::get_client_name(&self.client_sessions, ctx.client_id) {
			Some(name) => RespValue::bulk_string(name),
			None => RespValue::null(),
		}
	}
}

pub struct ClientListCommand {
	meta: CmdMeta,
	client_sessions: ClientSessions,
}

impl ClientListCommand {
	fn new(client_sessions: ClientSessions) -> Self {
		Self {
			meta: CmdMeta {
				name: "LIST".to_string(),
				arity: 1,
			},
			client_sessions,
		}
	}
}

#[async_trait]
impl Cmd for ClientListCommand {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, _args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let lines = client::list_clients(&self.client_sessions)
			.into_iter()
			.map(|(client_id, name)| {
				let name = name
					.map(|v| String::from_utf8_lossy(&v).into_owned())
					.unwrap_or_default();
				format!("id={} name={}", client_id, name)
			})
			.collect::<Vec<_>>()
			.join("\n");

		RespValue::bulk_string(Bytes::from(lines))
	}
}