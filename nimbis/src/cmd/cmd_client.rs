use std::collections::HashMap;

use async_trait::async_trait;
use bytes::Bytes;
use nimbis_resp::RespValue;
use nimbis_storage::Storage;

use super::Cmd;
use super::CmdContext;
use super::CmdMeta;
use crate::GCTX;

/// Client command implementation.
pub struct ClientCmd {
	meta: CmdMeta,
	sub_cmds: HashMap<&'static str, Box<dyn Cmd>>,
}

impl Default for ClientCmd {
	fn default() -> Self {
		let mut sub_cmds: HashMap<&'static str, Box<dyn Cmd>> = HashMap::new();

		sub_cmds.insert("ID", Box::new(ClientIdCmd::default()));
		sub_cmds.insert("SETNAME", Box::new(ClientSetNameCmd::default()));
		sub_cmds.insert("GETNAME", Box::new(ClientGetNameCmd::default()));
		sub_cmds.insert("LIST", Box::new(ClientListCmd::default()));

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
impl Cmd for ClientCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, storage: &Storage, args: &[Bytes], ctx: &CmdContext) -> RespValue {
		let sub_cmd_name = String::from_utf8_lossy(&args[0]).to_uppercase();
		match self.sub_cmds.get(sub_cmd_name.as_str()) {
			Some(sub_cmd) => sub_cmd.execute(storage, &args[1..], ctx).await,
			None => RespValue::error(format!("ERR unknown CLIENT subcommand '{}'", sub_cmd_name)),
		}
	}
}

pub struct ClientIdCmd {
	meta: CmdMeta,
}

impl Default for ClientIdCmd {
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
impl Cmd for ClientIdCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, _args: &[Bytes], ctx: &CmdContext) -> RespValue {
		RespValue::integer(ctx.client_id)
	}
}

pub struct ClientSetNameCmd {
	meta: CmdMeta,
}

impl Default for ClientSetNameCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "SETNAME".to_string(),
				arity: 2,
			},
		}
	}
}

#[async_trait]
impl Cmd for ClientSetNameCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, args: &[Bytes], ctx: &CmdContext) -> RespValue {
		if GCTX!(client_sessions).set_name(ctx.client_id, args[0].clone()) {
			RespValue::simple_string("OK")
		} else {
			RespValue::error("ERR client not found")
		}
	}
}

pub struct ClientGetNameCmd {
	meta: CmdMeta,
}

impl Default for ClientGetNameCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "GETNAME".to_string(),
				arity: 1,
			},
		}
	}
}

#[async_trait]
impl Cmd for ClientGetNameCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, _args: &[Bytes], ctx: &CmdContext) -> RespValue {
		match GCTX!(client_sessions).get_name(ctx.client_id) {
			Some(name) => RespValue::bulk_string(name),
			None => RespValue::null(),
		}
	}
}

pub struct ClientListCmd {
	meta: CmdMeta,
}

impl Default for ClientListCmd {
	fn default() -> Self {
		Self {
			meta: CmdMeta {
				name: "LIST".to_string(),
				arity: 1,
			},
		}
	}
}

#[async_trait]
impl Cmd for ClientListCmd {
	fn meta(&self) -> &CmdMeta {
		&self.meta
	}

	async fn do_cmd(&self, _storage: &Storage, _args: &[Bytes], _ctx: &CmdContext) -> RespValue {
		let lines = GCTX!(client_sessions)
			.list()
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
