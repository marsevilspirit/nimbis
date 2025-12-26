use crate::cmd::Db;
use resp::RespValue;

/// GET command implementation
pub struct GetCommand {
    key: String,
}

impl GetCommand {
    pub fn from_args(args: Vec<String>) -> Result<Self, String> {
        if args.len() != 1 {
            return Err("ERR wrong number of arguments for 'get' command".to_string());
        }

        Ok(GetCommand {
            key: args[0].clone(),
        })
    }

    pub async fn execute(&self, db: &Db) -> RespValue {
        let db = db.read().await;
        match db.get(&self.key) {
            Some(value) => RespValue::bulk_string(value.clone()),
            None => RespValue::Null,
        }
    }
}
