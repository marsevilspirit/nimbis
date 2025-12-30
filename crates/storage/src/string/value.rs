use bytes::Bytes;

#[derive(Debug, PartialEq, Clone)]
pub struct StringValue {
	pub value: Bytes,
}

impl StringValue {
	pub fn new(value: impl Into<Bytes>) -> Self {
		Self {
			value: value.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		self.value.clone()
	}

	pub fn decode(bytes: &[u8]) -> Self {
		Self::new(Bytes::copy_from_slice(bytes))
	}
}

impl From<Bytes> for StringValue {
	fn from(value: Bytes) -> Self {
		Self::new(value)
	}
}

impl From<&str> for StringValue {
	fn from(value: &str) -> Self {
		Self::new(Bytes::copy_from_slice(value.as_bytes()))
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("hello world")]
	#[case("")]
	#[case("test value")]
	fn test_roundtrip(#[case] input: &str) {
		let original = StringValue::new(Bytes::copy_from_slice(input.as_bytes()));
		let encoded = original.encode();
		let decoded = StringValue::decode(&encoded);
		assert_eq!(original, decoded);
	}
}
