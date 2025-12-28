use crate::cmd::{Cmd, CmdMeta};
use async_trait::async_trait;
use resp::RespValue;
use std::sync::Arc;
use storage::Storage;

/// SET command implementation
pub struct SetCommand {
    meta: CmdMeta,
}

impl Default for SetCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl SetCommand {
    pub fn new() -> Self {
        Self {
            meta: CmdMeta {
                name: "SET".to_string(),
                arity: 2,
            },
        }
    }
}

#[async_trait]
impl Cmd for SetCommand {
    fn meta(&self) -> &CmdMeta {
        &self.meta
    }

    async fn do_cmd(&self, storage: &Arc<dyn Storage>, args: &[String]) -> RespValue {
        let key = &args[0];
        let value = &args[1];

        match storage.set(key, value).await {
            Ok(_) => RespValue::simple_string("OK"),
            Err(e) => RespValue::error(format!("ERR {}", e)),
        }
    }
}
