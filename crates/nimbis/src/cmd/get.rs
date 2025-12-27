use crate::cmd::{Cmd, CmdMeta, Db};
use async_trait::async_trait;
use resp::RespValue;

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

    async fn do_cmd(&self, db: &Db, args: &[String]) -> RespValue {
        let key = &args[0];

        let db = db.read().await;
        match db.get(key) {
            Some(value) => RespValue::bulk_string(value.clone()),
            None => RespValue::Null,
        }
    }
}
