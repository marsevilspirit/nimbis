use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::data_type::DataType;
use crate::error::DecoderError;

#[derive(Debug, PartialEq)]
pub struct MetaKey {
	user_key: Bytes,
}

impl MetaKey {
	pub fn new(user_key: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		self.user_key.clone()
	}
}

#[derive(Debug, PartialEq)]
pub struct HashMetaValue {
	pub len: u64,
}

impl HashMetaValue {
	pub fn new(len: u64) -> Self {
		Self { len }
	}

	pub fn encode(&self) -> Bytes {
		let mut bytes = BytesMut::with_capacity(9);
		bytes.put_u8(DataType::Hash as u8);
		bytes.put_u64(self.len);
		bytes.freeze()
	}

	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.len() < 9 {
			return Err(DecoderError::InvalidLength);
		}

		let mut buf = bytes;
		let type_code = buf.get_u8();
		if type_code != DataType::Hash as u8 {
			return Err(DecoderError::InvalidType);
		}
		let len = buf.get_u64();
		Ok(Self::new(len))
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("mykey", b"mykey")]
	#[case("", b"")]
	fn test_meta_key_encode(#[case] key: &str, #[case] expected: &[u8]) {
		let meta_key = MetaKey::new(Bytes::copy_from_slice(key.as_bytes()));
		assert_eq!(&meta_key.encode()[..], expected);
	}

	#[test]
	fn test_hash_meta_value_encode() {
		let val = HashMetaValue::new(10);
		let encoded = val.encode();
		assert_eq!(encoded.len(), 9);
		assert_eq!(encoded[0], b'h');
		assert_eq!(&encoded[1..], &10u64.to_be_bytes());
	}

	#[test]
	fn test_hash_meta_value_decode() {
		let val = HashMetaValue::new(12345);
		let encoded = val.encode();
		let decoded = HashMetaValue::decode(&encoded).unwrap();
		assert_eq!(decoded, val);
	}
}
