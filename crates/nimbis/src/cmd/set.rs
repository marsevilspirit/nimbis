use crate::cmd::{Cmd, CmdMeta, Db};
use async_trait::async_trait;
use resp::RespValue;

/// SET command implementation
pub struct SetCommand {
    meta: CmdMeta,
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

    async fn do_cmd(&self, db: &Db, args: &[String]) -> RespValue {
        let key = &args[0];
        let value = &args[1];

        let mut db = db.write().await;
        db.insert(key.clone(), value.clone());
        RespValue::simple_string("OK")
    }
}
