use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::data_type::DataType;
use crate::error::DecoderError;
use crate::expirable::Expirable;

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
	pub expire_time: u64,
}

impl HashMetaValue {
	pub fn new(len: u64) -> Self {
		Self {
			len,
			expire_time: 0,
		}
	}

	pub fn new_with_ttl(len: u64, expire_time: u64) -> Self {
		Self { len, expire_time }
	}

	pub fn encode(&self) -> Bytes {
		let mut bytes = BytesMut::with_capacity(1 + 8 + 8);
		bytes.put_u8(DataType::Hash as u8);
		bytes.put_u64(self.len);
		bytes.put_u64(self.expire_time);
		bytes.freeze()
	}

	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.len() < 17 {
			return Err(DecoderError::InvalidLength);
		}

		let mut buf = bytes;
		let type_code = buf.get_u8();
		if type_code != DataType::Hash as u8 {
			return Err(DecoderError::InvalidType);
		}
		let len = buf.get_u64();
		let expire_time = buf.get_u64();
		Ok(Self::new_with_ttl(len, expire_time))
	}
}

impl Expirable for HashMetaValue {
	fn expire_time(&self) -> u64 {
		self.expire_time
	}

	fn set_expire_time(&mut self, timestamp: u64) {
		self.expire_time = timestamp;
	}
}

#[derive(Debug, PartialEq)]
pub struct ListMetaValue {
	pub len: u64,
	pub head: u64,
	pub tail: u64,
	pub expire_time: u64,
}

impl ListMetaValue {
	pub fn new() -> Self {
		// Initialize head and tail at the middle of u64 range to allow expansion in both directions
		let mid = u64::MAX / 2;
		Self {
			len: 0,
			head: mid,
			tail: mid,
			expire_time: 0,
		}
	}

	pub fn encode(&self) -> Bytes {
		let mut bytes = BytesMut::with_capacity(1 + 8 + 8 + 8 + 8);
		bytes.put_u8(DataType::List as u8);
		bytes.put_u64(self.len);
		bytes.put_u64(self.head);
		bytes.put_u64(self.tail);
		bytes.put_u64(self.expire_time);
		bytes.freeze()
	}

	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.len() < 33 {
			return Err(DecoderError::InvalidLength);
		}

		let mut buf = bytes;
		let type_code = buf.get_u8();
		if type_code != DataType::List as u8 {
			return Err(DecoderError::InvalidType);
		}
		let len = buf.get_u64();
		let head = buf.get_u64();
		let tail = buf.get_u64();
		let expire_time = buf.get_u64();
		Ok(Self {
			len,
			head,
			tail,
			expire_time,
		})
	}
}

impl Default for ListMetaValue {
	fn default() -> Self {
		Self::new()
	}
}

impl Expirable for ListMetaValue {
	fn expire_time(&self) -> u64 {
		self.expire_time
	}

	fn set_expire_time(&mut self, timestamp: u64) {
		self.expire_time = timestamp;
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
		let val = HashMetaValue::new_with_ttl(10, 123456789);
		let encoded = val.encode();
		assert_eq!(encoded.len(), 17);
		assert_eq!(encoded[0], b'h');
		assert_eq!(&encoded[1..9], &10u64.to_be_bytes());
		assert_eq!(&encoded[9..17], &123456789u64.to_be_bytes());
	}

	#[test]
	fn test_hash_meta_value_decode() {
		let val = HashMetaValue::new_with_ttl(12345, 987654321);
		let encoded = val.encode();
		let decoded = HashMetaValue::decode(&encoded).unwrap();
		assert_eq!(decoded, val);
	}

	#[test]
	fn test_hash_meta_value_new() {
		let val = HashMetaValue::new(100);
		assert_eq!(val.len, 100);
		assert_eq!(val.expire_time, 0);

		let encoded = val.encode();
		let decoded = HashMetaValue::decode(&encoded).unwrap();
		assert_eq!(decoded, val);
		assert_eq!(decoded.expire_time, 0);
	}

	#[test]
	fn test_list_meta_value_encode_decode() {
		let mut val = ListMetaValue::new();
		val.len = 5;
		val.head = 100;
		val.tail = 105;
		val.expire_time = 123456789;

		let encoded = val.encode();
		let decoded = ListMetaValue::decode(&encoded).unwrap();
		assert_eq!(decoded, val);
	}

	#[test]
	fn test_list_meta_value_new() {
		let val = ListMetaValue::new();
		assert_eq!(val.len, 0);
		// Approx checking mid range
		assert!(val.head > 0);
		assert_eq!(val.head, val.tail);
	}
}
