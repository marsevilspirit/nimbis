use crate::cmd::{Cmd, CmdMeta};
use async_trait::async_trait;
use resp::RespValue;
use std::sync::Arc;
use storage::Storage;

/// PING command implementation
pub struct PingCommand {
    meta: CmdMeta,
}

impl Default for PingCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl PingCommand {
    pub fn new() -> Self {
        Self {
            meta: CmdMeta {
                name: "PING".to_string(),
                arity: -1, // Allow 0 or 1 argument
            },
        }
    }
}

#[async_trait]
impl Cmd for PingCommand {
    fn meta(&self) -> &CmdMeta {
        &self.meta
    }

    async fn do_cmd(&self, _storage: &Arc<Storage>, args: &[bytes::Bytes]) -> RespValue {
        match args.len() {
            0 => RespValue::simple_string("PONG"),
            1 => RespValue::bulk_string(args[0].clone()),
            _ => RespValue::error("ERR wrong number of arguments for 'ping' command"),
        }
    }
}
