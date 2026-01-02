use bytes::Bytes;

use crate::error::DecoderError;

#[derive(Debug, PartialEq)]
pub struct StringKey {
	user_key: Bytes,
}

impl StringKey {
	pub fn new(user_key: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		self.user_key.clone()
	}

	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.is_empty() {
			return Err(DecoderError::Empty);
		}
		Ok(Self::new(Bytes::copy_from_slice(bytes)))
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("mykey", b"mykey")]
	#[case("something else", b"something else")]
	fn test_encode(#[case] key: &str, #[case] expected: &[u8]) {
		let key = StringKey::new(Bytes::copy_from_slice(key.as_bytes()));
		let encoded = key.encode();
		assert_eq!(&encoded[..], expected);
	}

	#[rstest]
	#[case(b"mykey", "mykey")]
	#[case(b"something else", "something else")]
	fn test_decode(#[case] encoded: &[u8], #[case] expected: &str) {
		let key = StringKey::decode(encoded).unwrap();
		assert_eq!(key.user_key, Bytes::copy_from_slice(expected.as_bytes()));
	}

	#[test]
	fn test_decode_empty() {
		let encoded = b"";
		let err = StringKey::decode(encoded).unwrap_err();
		assert!(matches!(err, DecoderError::Empty));
	}
}
