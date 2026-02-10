use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::data_type::DataType;
use crate::error::DecoderError;
use crate::expirable::Expirable;
use crate::string::value::StringValue;

/// Trait for values stored in the string database that carry TTL and type
/// information.
pub trait MetaValue: Expirable + Sized {
	/// Decode the value from bytes.
	fn decode(bytes: &[u8]) -> Result<Self, DecoderError>;
	/// Check if the given type code matches this meta value type.
	fn is_type_match(type_code: u8) -> bool;
	/// Encode the value to bytes.
	fn encode(&self) -> Bytes;
	/// Return the expected data type for this meta value, if specific.
	/// Used for better error messages on type mismatch.
	fn data_type() -> Option<DataType> {
		None
	}
}

impl MetaValue for StringValue {
	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn is_type_match(type_code: u8) -> bool {
		type_code == DataType::String as u8
	}

	fn data_type() -> Option<DataType> {
		Some(DataType::String)
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}
}

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
		let mut buf = BytesMut::with_capacity(2 + self.user_key.len());
		buf.put_u16(self.user_key.len() as u16);
		buf.extend_from_slice(&self.user_key);
		buf.freeze()
	}
}

#[derive(Debug, PartialEq)]
pub struct HashMetaValue {
	pub version: u64,
	pub len: u64,
	pub expire_time: u64,
}

impl HashMetaValue {
	pub fn new(version: u64, len: u64) -> Self {
		Self {
			version,
			len,
			expire_time: 0,
		}
	}

	pub fn new_with_ttl(version: u64, len: u64, expire_time: u64) -> Self {
		Self {
			version,
			len,
			expire_time,
		}
	}

	pub fn encode(&self) -> Bytes {
		let mut bytes = BytesMut::with_capacity(1 + 8 + 8 + 8);
		bytes.put_u8(DataType::Hash as u8);
		bytes.put_u64(self.version);
		bytes.put_u64(self.len);
		bytes.put_u64(self.expire_time);
		bytes.freeze()
	}

	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.len() < 25 {
			return Err(DecoderError::InvalidLength);
		}

		let mut buf = bytes;
		let type_code = buf.get_u8();
		if type_code != DataType::Hash as u8 {
			return Err(DecoderError::InvalidType);
		}
		let version = buf.get_u64();
		let len = buf.get_u64();
		let expire_time = buf.get_u64();
		Ok(Self::new_with_ttl(version, len, expire_time))
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

impl MetaValue for HashMetaValue {
	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn is_type_match(type_code: u8) -> bool {
		type_code == DataType::Hash as u8
	}

	fn data_type() -> Option<DataType> {
		Some(DataType::Hash)
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}
}

#[derive(Debug, PartialEq)]
pub struct ListMetaValue {
	pub version: u64,
	pub len: u64,
	pub head: u64,
	pub tail: u64,
	pub expire_time: u64,
}

impl ListMetaValue {
	pub fn new(version: u64) -> Self {
		// Initialize head and tail at the middle of u64 range to allow expansion in
		// both directions
		let mid = u64::MAX / 2;
		Self {
			version,
			len: 0,
			head: mid,
			tail: mid,
			expire_time: 0,
		}
	}

	pub fn encode(&self) -> Bytes {
		let mut bytes = BytesMut::with_capacity(1 + 8 + 8 + 8 + 8 + 8);
		bytes.put_u8(DataType::List as u8);
		bytes.put_u64(self.version);
		bytes.put_u64(self.len);
		bytes.put_u64(self.head);
		bytes.put_u64(self.tail);
		bytes.put_u64(self.expire_time);
		bytes.freeze()
	}

	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.len() < 41 {
			return Err(DecoderError::InvalidLength);
		}

		let mut buf = bytes;
		let type_code = buf.get_u8();
		if type_code != DataType::List as u8 {
			return Err(DecoderError::InvalidType);
		}
		let version = buf.get_u64();
		let len = buf.get_u64();
		let head = buf.get_u64();
		let tail = buf.get_u64();
		let expire_time = buf.get_u64();
		Ok(Self {
			version,
			len,
			head,
			tail,
			expire_time,
		})
	}
}

impl Default for ListMetaValue {
	fn default() -> Self {
		Self::new(0)
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

impl MetaValue for ListMetaValue {
	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn is_type_match(type_code: u8) -> bool {
		type_code == DataType::List as u8
	}

	fn data_type() -> Option<DataType> {
		Some(DataType::List)
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}
}

#[derive(Debug, PartialEq)]
pub struct SetMetaValue {
	pub version: u64,
	pub len: u64,
	pub expire_time: u64,
}

impl SetMetaValue {
	pub fn new(version: u64, len: u64) -> Self {
		Self {
			version,
			len,
			expire_time: 0,
		}
	}

	pub fn new_with_ttl(version: u64, len: u64, expire_time: u64) -> Self {
		Self {
			version,
			len,
			expire_time,
		}
	}

	pub fn encode(&self) -> Bytes {
		let mut bytes = BytesMut::with_capacity(1 + 8 + 8 + 8);
		bytes.put_u8(DataType::Set as u8);
		bytes.put_u64(self.version);
		bytes.put_u64(self.len);
		bytes.put_u64(self.expire_time);
		bytes.freeze()
	}

	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.len() < 25 {
			return Err(DecoderError::InvalidLength);
		}

		let mut buf = bytes;
		let type_code = buf.get_u8();
		if type_code != DataType::Set as u8 {
			return Err(DecoderError::InvalidType);
		}
		let version = buf.get_u64();
		let len = buf.get_u64();
		let expire_time = buf.get_u64();
		Ok(Self::new_with_ttl(version, len, expire_time))
	}
}

impl Expirable for SetMetaValue {
	fn expire_time(&self) -> u64 {
		self.expire_time
	}

	fn set_expire_time(&mut self, timestamp: u64) {
		self.expire_time = timestamp;
	}
}

impl MetaValue for SetMetaValue {
	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn is_type_match(type_code: u8) -> bool {
		type_code == DataType::Set as u8
	}

	fn data_type() -> Option<DataType> {
		Some(DataType::Set)
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}
}

#[derive(Debug, PartialEq)]
pub struct ZSetMetaValue {
	pub version: u64,
	pub len: u64,
	pub expire_time: u64,
}

impl ZSetMetaValue {
	pub fn new(version: u64, len: u64) -> Self {
		Self {
			version,
			len,
			expire_time: 0,
		}
	}

	pub fn new_with_ttl(version: u64, len: u64, expire_time: u64) -> Self {
		Self {
			version,
			len,
			expire_time,
		}
	}

	pub fn encode(&self) -> Bytes {
		let mut bytes = BytesMut::with_capacity(1 + 8 + 8 + 8);
		bytes.put_u8(DataType::ZSet as u8);
		bytes.put_u64(self.version);
		bytes.put_u64(self.len);
		bytes.put_u64(self.expire_time);
		bytes.freeze()
	}

	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.len() < 25 {
			return Err(DecoderError::InvalidLength);
		}

		let mut buf = bytes;
		let type_code = buf.get_u8();
		if type_code != DataType::ZSet as u8 {
			return Err(DecoderError::InvalidType);
		}
		let version = buf.get_u64();
		let len = buf.get_u64();
		let expire_time = buf.get_u64();
		Ok(Self::new_with_ttl(version, len, expire_time))
	}
}

impl Expirable for ZSetMetaValue {
	fn expire_time(&self) -> u64 {
		self.expire_time
	}

	fn set_expire_time(&mut self, timestamp: u64) {
		self.expire_time = timestamp;
	}
}

impl MetaValue for ZSetMetaValue {
	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn is_type_match(type_code: u8) -> bool {
		type_code == DataType::ZSet as u8
	}

	fn data_type() -> Option<DataType> {
		Some(DataType::ZSet)
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}
}

/// Enum representing any value or metadata stored in the string database.
pub enum AnyValue {
	String(StringValue),
	Hash(HashMetaValue),
	List(ListMetaValue),
	Set(SetMetaValue),
	ZSet(ZSetMetaValue),
}

impl AnyValue {
	pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		if bytes.is_empty() {
			return Err(DecoderError::Empty);
		}
		match DataType::from_u8(bytes[0]) {
			Some(DataType::String) => Ok(Self::String(StringValue::decode(bytes)?)),
			Some(DataType::Hash) => Ok(Self::Hash(HashMetaValue::decode(bytes)?)),
			Some(DataType::List) => Ok(Self::List(ListMetaValue::decode(bytes)?)),
			Some(DataType::Set) => Ok(Self::Set(SetMetaValue::decode(bytes)?)),
			Some(DataType::ZSet) => Ok(Self::ZSet(ZSetMetaValue::decode(bytes)?)),
			None => Err(DecoderError::InvalidType),
		}
	}

	pub fn data_type(&self) -> DataType {
		match self {
			Self::String(_) => DataType::String,
			Self::Hash(_) => DataType::Hash,
			Self::List(_) => DataType::List,
			Self::Set(_) => DataType::Set,
			Self::ZSet(_) => DataType::ZSet,
		}
	}

	pub fn encode(&self) -> Bytes {
		match self {
			Self::String(v) => v.encode(),
			Self::Hash(v) => v.encode(),
			Self::List(v) => v.encode(),
			Self::Set(v) => v.encode(),
			Self::ZSet(v) => v.encode(),
		}
	}

	pub fn version(&self) -> Option<u64> {
		match self {
			Self::String(_) => None,
			Self::Hash(v) => Some(v.version),
			Self::List(v) => Some(v.version),
			Self::Set(v) => Some(v.version),
			Self::ZSet(v) => Some(v.version),
		}
	}
}

impl Expirable for AnyValue {
	fn expire_time(&self) -> u64 {
		match self {
			Self::String(v) => v.expire_time(),
			Self::Hash(v) => v.expire_time(),
			Self::List(v) => v.expire_time(),
			Self::Set(v) => v.expire_time(),
			Self::ZSet(v) => v.expire_time(),
		}
	}

	fn set_expire_time(&mut self, timestamp: u64) {
		match self {
			Self::String(v) => v.set_expire_time(timestamp),
			Self::Hash(v) => v.set_expire_time(timestamp),
			Self::List(v) => v.set_expire_time(timestamp),
			Self::Set(v) => v.set_expire_time(timestamp),
			Self::ZSet(v) => v.set_expire_time(timestamp),
		}
	}
}

impl From<StringValue> for AnyValue {
	fn from(v: StringValue) -> Self {
		Self::String(v)
	}
}

impl From<HashMetaValue> for AnyValue {
	fn from(v: HashMetaValue) -> Self {
		Self::Hash(v)
	}
}

impl From<ListMetaValue> for AnyValue {
	fn from(v: ListMetaValue) -> Self {
		Self::List(v)
	}
}

impl From<SetMetaValue> for AnyValue {
	fn from(v: SetMetaValue) -> Self {
		Self::Set(v)
	}
}

impl From<ZSetMetaValue> for AnyValue {
	fn from(v: ZSetMetaValue) -> Self {
		Self::ZSet(v)
	}
}

impl MetaValue for AnyValue {
	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn is_type_match(_type_code: u8) -> bool {
		true
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("mykey", b"\x00\x05mykey")]
	#[case("", b"\x00\x00")]
	fn test_meta_key_encode(#[case] key: &str, #[case] expected: &[u8]) {
		let meta_key = MetaKey::new(Bytes::copy_from_slice(key.as_bytes()));
		assert_eq!(&meta_key.encode()[..], expected);
	}

	#[test]
	fn test_hash_meta_value_encode() {
		let val = HashMetaValue::new_with_ttl(1, 10, 123456789);
		let encoded = val.encode();
		assert_eq!(encoded.len(), 25);
		assert_eq!(encoded[0], b'h');
		assert_eq!(&encoded[1..9], &1u64.to_be_bytes());
		assert_eq!(&encoded[9..17], &10u64.to_be_bytes());
		assert_eq!(&encoded[17..25], &123456789u64.to_be_bytes());
	}

	#[test]
	fn test_hash_meta_value_decode() {
		let val = HashMetaValue::new_with_ttl(1, 12345, 987654321);
		let encoded = val.encode();
		let decoded = HashMetaValue::decode(&encoded).unwrap();
		assert_eq!(decoded, val);
	}

	#[test]
	fn test_hash_meta_value_new() {
		let val = HashMetaValue::new(1, 100);
		assert_eq!(val.version, 1);
		assert_eq!(val.len, 100);
		assert_eq!(val.expire_time, 0);

		let encoded = val.encode();
		let decoded = HashMetaValue::decode(&encoded).unwrap();
		assert_eq!(decoded, val);
		assert_eq!(decoded.expire_time, 0);
	}

	#[test]
	fn test_set_meta_value_encode() {
		let val = SetMetaValue::new_with_ttl(1, 5, 111222333);
		let encoded = val.encode();
		assert_eq!(encoded.len(), 25);
		assert_eq!(encoded[0], b'S');
		assert_eq!(&encoded[1..9], &1u64.to_be_bytes());
		assert_eq!(&encoded[9..17], &5u64.to_be_bytes());
		assert_eq!(&encoded[17..25], &111222333u64.to_be_bytes());
	}

	#[test]
	fn test_set_meta_value_decode() {
		let val = SetMetaValue::new_with_ttl(1, 555, 999888);
		let encoded = val.encode();
		let decoded = SetMetaValue::decode(&encoded).unwrap();
		assert_eq!(decoded, val);
	}

	#[test]
	fn test_set_meta_value_new() {
		let val = SetMetaValue::new(1, 50);
		assert_eq!(val.version, 1);
		assert_eq!(val.len, 50);
		assert_eq!(val.expire_time, 0);

		let encoded = val.encode();
		let decoded = SetMetaValue::decode(&encoded).unwrap();
		assert_eq!(decoded, val);
	}

	#[test]
	fn test_list_meta_value_encode_decode() {
		let mut val = ListMetaValue::new(1);
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
		let val = ListMetaValue::new(1);
		assert_eq!(val.version, 1);
		assert_eq!(val.len, 0);
		// Approx checking mid range
		assert!(val.head > 0);
		assert_eq!(val.head, val.tail);
	}

	#[test]
	fn test_zset_meta_value_encode() {
		let val = ZSetMetaValue::new_with_ttl(1, 5, 111222333);
		let encoded = val.encode();
		assert_eq!(encoded.len(), 25);
		assert_eq!(encoded[0], b'z');
		assert_eq!(&encoded[1..9], &1u64.to_be_bytes());
		assert_eq!(&encoded[9..17], &5u64.to_be_bytes());
		assert_eq!(&encoded[17..25], &111222333u64.to_be_bytes());
	}

	#[test]
	fn test_zset_meta_value_decode() {
		let val = ZSetMetaValue::new_with_ttl(1, 555, 999888);
		let encoded = val.encode();
		let decoded = ZSetMetaValue::decode(&encoded).unwrap();
		assert_eq!(decoded, val);
	}
}
