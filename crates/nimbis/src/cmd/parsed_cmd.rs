use resp::RespValue;

/// Parsed command structure (renamed from Cmd to avoid conflict)
pub struct ParsedCmd {
	pub name: String,
	pub args: Vec<bytes::Bytes>,
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
		let cmd_args: Result<Vec<bytes::Bytes>, _> = args[1..]
			.iter()
			.map(|v| v.as_bytes().cloned().ok_or("Invalid argument"))
			.collect();

		Ok(ParsedCmd {
			name: cmd_name,
			args: cmd_args?,
		})
	}
}
