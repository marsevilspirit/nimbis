use async_trait::async_trait;
use resp::RespValue;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

mod get;
mod set;

pub use get::GetCommand;
pub use set::SetCommand;

pub type Db = Arc<RwLock<HashMap<String, String>>>;
pub type CmdTable = HashMap<String, Arc<dyn Cmd>>;

/// Command metadata containing immutable information about a command
#[derive(Debug, Clone, Default)]
pub struct CmdMeta {
    pub name: String,
    pub arity: i16,
}

impl CmdMeta {
    /// Validate argument count against arity
    /// - Positive arity: requires exact match
    /// - Negative arity: allows up to abs(arity) arguments
    pub fn validate_arity(&self, arg_count: usize) -> Result<(), String> {
        if self.arity > 0 {
            // Positive: exact match required
            if arg_count != self.arity as usize {
                return Err(format!(
                    "ERR wrong number of arguments for '{}' command",
                    self.name.to_lowercase()
                ));
            }
        } else if self.arity < 0 {
            // Negative: maximum count allowed
            let max_args = (-self.arity) as usize;
            if arg_count > max_args {
                return Err(format!(
                    "ERR too many arguments for '{}' command (max {})",
                    self.name.to_lowercase(),
                    max_args
                ));
            }
        }
        // arity == 0 means any number of arguments is allowed
        Ok(())
    }
}

/// Command trait - all commands must implement this
#[async_trait]
pub trait Cmd: Send + Sync {
    /// Get command metadata
    fn meta(&self) -> &CmdMeta;

    fn validate_arity(&self, arg_count: usize) -> Result<(), String> {
        self.meta().validate_arity(arg_count)
    }

    async fn do_cmd(&self, db: &Db, args: &[String]) -> RespValue;

    /// Execute the command
    async fn execute(&self, db: &Db, args: &[String]) -> RespValue {
        if let Err(err) = self.validate_arity(args.len()) {
            return RespValue::error(err);
        }

        self.do_cmd(db, args).await
    }
}

/// Global command table storing command instances
static CMD_TABLE: OnceLock<CmdTable> = OnceLock::new();

/// Initialize the global command table with all available commands
fn init_cmd_table() -> CmdTable {
    let mut table: CmdTable = HashMap::new();

    table.insert("SET".to_string(), Arc::new(SetCommand::new()));
    table.insert("GET".to_string(), Arc::new(GetCommand::new()));

    table
}

/// Get reference to the global command table
pub fn get_cmd_table() -> &'static CmdTable {
    CMD_TABLE.get_or_init(init_cmd_table)
}

/// Parsed command structure (renamed from Cmd to avoid conflict)
pub struct ParsedCmd {
    pub name: String,
    pub args: Vec<String>,
}

impl TryFrom<RespValue> for ParsedCmd {
    type Error = String;

    fn try_from(value: RespValue) -> Result<Self, Self::Error> {
        // RespValue should be an array
        let args = value.as_array().ok_or("Expected array")?;

        if args.is_empty() {
            return Err("Empty command".to_string());
        }

        // First element is the command name
        let cmd_name = args[0]
            .as_str()
            .ok_or("Invalid command type")?
            .to_uppercase();

        // Remaining elements are arguments
        let cmd_args: Result<Vec<String>, _> = args[1..]
            .iter()
            .map(|v| v.as_str().map(|s| s.to_string()).ok_or("Invalid argument"))
            .collect();

        Ok(ParsedCmd {
            name: cmd_name,
            args: cmd_args?,
        })
    }
}
