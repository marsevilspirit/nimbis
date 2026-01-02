use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::data_type::DataType;
use crate::error::DecoderError;

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
		// [Type: 's'] [Value]
		let mut bytes = BytesMut::with_capacity(1 + self.value.len());
		bytes.put_u8(DataType::String as u8);
		bytes.extend_from_slice(&self.value);
		bytes.freeze()
	}

	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.is_empty() {
			return Err(DecoderError::Empty);
		}
		if bytes[0] != DataType::String as u8 {
			return Err(DecoderError::InvalidType);
		}
		Ok(Self::new(Bytes::copy_from_slice(&bytes[1..])))
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
		assert_eq!(encoded[0], DataType::String as u8);
		let decoded = StringValue::decode(&encoded).unwrap();
		assert_eq!(original, decoded);
	}

	#[test]
	fn test_decode_invalid_type() {
		let bytes = b"h\x00\x00\x00\x00\x00\x00\x00\x01"; // Hash meta value
		let err = StringValue::decode(bytes).unwrap_err();
		assert!(matches!(err, DecoderError::InvalidType));
	}
}
