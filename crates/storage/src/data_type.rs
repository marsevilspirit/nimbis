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
