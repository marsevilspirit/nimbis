use crate::cmd::{Cmd, CmdMeta};
use async_trait::async_trait;
use resp::RespValue;
use std::sync::Arc;
use storage::Storage;

/// GET command implementation
pub struct GetCommand {
    meta: CmdMeta,
}

impl Default for GetCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl GetCommand {
    pub fn new() -> Self {
        Self {
            meta: CmdMeta {
                name: "GET".to_string(),
                arity: 1,
            },
        }
    }
}

#[async_trait]
impl Cmd for GetCommand {
    fn meta(&self) -> &CmdMeta {
        &self.meta
    }

    async fn do_cmd(&self, storage: &Arc<Storage>, args: &[String]) -> RespValue {
        let key = &args[0];

        match storage.get(key).await {
            Ok(Some(value)) => RespValue::bulk_string(value),
            Ok(None) => RespValue::Null,
            Err(e) => RespValue::error(format!("ERR {}", e)),
        }
    }
}
