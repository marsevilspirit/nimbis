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
			// Negative: minimum count allowed
			let min_args = (-self.arity) as usize;
			if arg_count < min_args {
				return Err(format!(
					"ERR wrong number of arguments for '{}' command",
					self.name.to_lowercase()
				));
			}
		}
		// arity == 0 means any number of arguments is allowed
		Ok(())
	}
}
