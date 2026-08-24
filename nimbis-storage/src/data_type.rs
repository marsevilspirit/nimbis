#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
	String = b's',
	Hash = b'h',
	Set = b'S',
	List = b'l',
	ZSet = b'z',
}

impl DataType {
	/// Parses the protocol-level type selector used by key-scoped commands.
	///
	/// Command names are case-insensitive, so type selectors follow the same
	/// rule. Keeping this parser next to `DataType` gives the command layer one
	/// canonical mapping to the five physical databases.
	pub fn from_name(value: &[u8]) -> Option<Self> {
		if value.eq_ignore_ascii_case(b"string") {
			Some(Self::String)
		} else if value.eq_ignore_ascii_case(b"hash") {
			Some(Self::Hash)
		} else if value.eq_ignore_ascii_case(b"list") {
			Some(Self::List)
		} else if value.eq_ignore_ascii_case(b"set") {
			Some(Self::Set)
		} else if value.eq_ignore_ascii_case(b"zset") {
			Some(Self::ZSet)
		} else {
			None
		}
	}

	pub fn from_u8(v: u8) -> Option<Self> {
		match v {
			b's' => Some(Self::String),
			b'h' => Some(Self::Hash),
			b'S' => Some(Self::Set),
			b'l' => Some(Self::List),
			b'z' => Some(Self::ZSet),
			_ => None,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_protocol_type_names_case_insensitively() {
		assert_eq!(DataType::from_name(b"STRING"), Some(DataType::String));
		assert_eq!(DataType::from_name(b"hash"), Some(DataType::Hash));
		assert_eq!(DataType::from_name(b"LiSt"), Some(DataType::List));
		assert_eq!(DataType::from_name(b"SET"), Some(DataType::Set));
		assert_eq!(DataType::from_name(b"zset"), Some(DataType::ZSet));
		assert_eq!(DataType::from_name(b"stream"), None);
	}
}
