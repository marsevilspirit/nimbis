use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::data_type::DataType;
use crate::error::DecoderError;
use crate::expirable::Expirable;

#[derive(Debug, PartialEq, Clone)]
pub struct StringValue {
	pub expire_time: u64,
	pub value: Bytes,
}

impl StringValue {
	pub fn new(value: impl Into<Bytes>) -> Self {
		Self {
			value: value.into(),
			expire_time: 0,
		}
	}

	pub fn new_with_ttl(value: impl Into<Bytes>, expire_time: u64) -> Self {
		Self {
			value: value.into(),
			expire_time,
		}
	}

	pub fn encode(&self) -> Bytes {
		// [Type: 's'] [expire_time: u64] [Value]
		let mut bytes = BytesMut::with_capacity(1 + 8 + self.value.len());
		bytes.put_u8(DataType::String as u8);
		bytes.put_u64(self.expire_time);
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
		if buf.len() < 8 {
			return Err(DecoderError::InvalidLength);
		}
		let expire_time = buf.get_u64();
		Ok(Self::new_with_ttl(Bytes::copy_from_slice(buf), expire_time))
	}
}

impl Expirable for StringValue {
	fn expire_time(&self) -> u64 {
		self.expire_time
	}

	fn set_expire_time(&mut self, timestamp: u64) {
		self.expire_time = timestamp;
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
	#[case("hello world", 0)]
	#[case("", 1000)]
	#[case("test value", 123456789)]
	fn test_roundtrip(#[case] input: &str, #[case] expire_time: u64) {
		let original =
			StringValue::new_with_ttl(Bytes::copy_from_slice(input.as_bytes()), expire_time);
		let encoded = original.encode();
		assert_eq!(encoded[0], DataType::String as u8);
		assert_eq!(&encoded[1..9], &expire_time.to_be_bytes());
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
	#[case(0, b"")]
	#[case(u64::MAX, b"val")]
	#[case(12345, b"\x00\xff\xaa\x55")]
	#[case(99999, b"normal value")]
	fn test_decode_edge_cases(#[case] expire_time: u64, #[case] value_bytes: &[u8]) {
		let mut buf = BytesMut::new();
		buf.put_u8(DataType::String as u8);
		buf.put_u64(expire_time);
		buf.extend_from_slice(value_bytes);

		let val = StringValue::decode(&buf.freeze()).unwrap();
		assert_eq!(val.expire_time, expire_time);
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

		// Invalid Length (header too short)
		let buf = b"s\x00\x00\x01"; // Type + partial u64
		let err = StringValue::decode(buf).unwrap_err();
		assert!(matches!(err, DecoderError::InvalidLength));
	}
}
