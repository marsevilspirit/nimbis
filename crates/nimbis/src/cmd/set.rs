use crate::cmd::Db;
use resp::RespValue;

/// SET command implementation
pub struct SetCommand {
    key: String,
    value: String,
}

impl SetCommand {
    pub fn from_args(args: Vec<String>) -> Result<Self, String> {
        if args.len() != 2 {
            return Err("ERR wrong number of arguments for 'set' command".to_string());
        }

        Ok(SetCommand {
            key: args[0].clone(),
            value: args[1].clone(),
        })
    }

    pub async fn execute(&self, db: &Db) -> RespValue {
        let mut db = db.write().await;
        db.insert(self.key.clone(), self.value.clone());
        RespValue::simple_string("OK")
    }
}
