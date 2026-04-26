use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

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
		let mut buf = BytesMut::with_capacity(2 + self.user_key.len());
		buf.put_u16(self.user_key.len() as u16);
		buf.extend_from_slice(&self.user_key);
		buf.freeze()
	}

	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.len() < 2 {
			return Err(DecoderError::InvalidLength);
		}
		let mut buf = bytes;
		let len = buf.get_u16() as usize;
		if buf.len() < len {
			return Err(DecoderError::InvalidLength);
		}
		let user_key = Bytes::copy_from_slice(&buf[..len]);
		Ok(Self::new(user_key))
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("mykey", b"\x00\x05mykey")]
	#[case("something else", b"\x00\x0esomething else")]
	fn test_encode(#[case] key: &str, #[case] expected: &[u8]) {
		let key = StringKey::new(Bytes::copy_from_slice(key.as_bytes()));
		let encoded = key.encode();
		assert_eq!(&encoded[..], expected);
	}

	#[rstest]
	#[case(b"\x00\x05mykey", "mykey")]
	#[case(b"\x00\x0esomething else", "something else")]
	fn test_decode(#[case] encoded: &[u8], #[case] expected: &str) {
		let key = StringKey::decode(encoded).unwrap();
		assert_eq!(key.user_key, Bytes::copy_from_slice(expected.as_bytes()));
	}

	#[test]
	fn test_decode_empty() {
		let encoded = b"";
		let err = StringKey::decode(encoded).unwrap_err();
		assert!(matches!(err, DecoderError::InvalidLength));
	}
}
