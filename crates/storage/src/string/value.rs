use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::data_type::DataType;
use crate::error::DecoderError;
use crate::expirable::Expirable;

#[derive(Debug, PartialEq, Clone)]
pub struct StringValue {
	pub value: Bytes,
}

impl StringValue {
	pub fn new(value: impl Into<Bytes>) -> Self {
		Self { value: value.into() }
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
		let mut buf = bytes;
		if buf.get_u8() != DataType::String as u8 {
			return Err(DecoderError::InvalidType);
		}
		Ok(Self::new(Bytes::copy_from_slice(buf)))
	}
}

impl Expirable for StringValue {
	fn expire_time(&self) -> u64 {
		0
	}

	fn set_expire_time(&mut self, _timestamp: u64) {
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

	#[rstest]
	#[case(b"")]
	#[case(b"val")]
	#[case(b"\x00\xff\xaa\x55")]
	#[case(b"normal value")]
	fn test_decode_edge_cases(#[case] value_bytes: &[u8]) {
		let mut buf = BytesMut::new();
		buf.put_u8(DataType::String as u8);
		buf.extend_from_slice(value_bytes);

		let val = StringValue::decode(&buf.freeze()).unwrap();
		assert_eq!(val.value, Bytes::copy_from_slice(value_bytes));
	}

	#[test]
	fn test_decode_errors() {
		// Empty input
		let err = StringValue::decode(b"").unwrap_err();
		assert!(matches!(err, DecoderError::Empty));

		// Invalid Type
		let err = StringValue::decode(b"x\x00\x00\x00\x00\x00\x00\x00\x01").unwrap_err();
		assert!(matches!(err, DecoderError::InvalidType));
	}
}
