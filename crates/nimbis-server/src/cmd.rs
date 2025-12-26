pub enum CmdType {
    SET,
    GET,
}

impl std::str::FromStr for CmdType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "SET" => Ok(CmdType::SET),
            "GET" => Ok(CmdType::GET),
            _ => Err(format!("Unknown command: {}", s)),
        }
    }
}

pub struct Cmd {
    pub typ: CmdType,
    pub args: Vec<String>,
}

impl TryFrom<resp::RespValue> for Cmd {
    type Error = String;

    fn try_from(value: resp::RespValue) -> Result<Self, Self::Error> {
        // RespValue should be an array
        let args = value.as_array().ok_or("Expected array")?;

        if args.is_empty() {
            return Err("Empty command".to_string());
        }

        // First element is the command
        let cmd_str = args[0].as_str().ok_or("Invalid command type")?;

        let cmd_type = cmd_str.parse::<CmdType>()?;

        // Remaining elements are arguments
        let cmd_args: Result<Vec<String>, _> = args[1..]
            .iter()
            .map(|v| v.as_str().map(|s| s.to_string()).ok_or("Invalid argument"))
            .collect();

        Ok(Cmd {
            typ: cmd_type,
            args: cmd_args?,
        })
    }
}
