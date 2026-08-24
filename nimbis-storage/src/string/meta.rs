use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::data_type::DataType;
use crate::error::DecoderError;
use crate::string::value::StringValue;

/// Common state carried by a decoded top-level row.
///
/// Concrete typed values implement this through [`TopLevelValue`], while
/// [`AnyValue`] implements it directly for lifecycle and migration code. This
/// keeps on-disk row normalization independent from the caller's decode mode.
pub(crate) trait TopLevelState: Sized {
	fn decode_state(bytes: &[u8]) -> Result<Self, DecoderError>;
	fn data_type(&self) -> DataType;
	fn embedded_expire_time(&self) -> Option<u64>;
	fn set_embedded_expire_time(&mut self, timestamp: u64);
	fn resolve_pending_generation(&mut self, row_sequence: u64);
}

/// Value stored in a typed database's top-level row.
pub(crate) trait TopLevelValue: Sized {
	const DATA_TYPE: DataType;
	const HAS_EMBEDDED_EXPIRATION: bool = true;

	/// Decode the value from bytes.
	fn decode(bytes: &[u8]) -> Result<Self, DecoderError>;
	/// Encode the value to bytes.
	fn encode(&self) -> Bytes;
	/// Get the expiration timestamp in milliseconds since Unix epoch.
	/// Returns 0 if no expiration is set.
	fn expire_time(&self) -> u64;
	/// Set the expiration timestamp in milliseconds since Unix epoch.
	fn set_expire_time(&mut self, timestamp: u64);
	/// Return the collection generation, if this value has one.
	fn generation(&self) -> Option<u64> {
		None
	}
	/// Replace the collection generation.
	fn set_generation(&mut self, _generation: u64) {}

	fn resolve_pending_generation(&mut self, row_sequence: u64) {
		if self.generation() == Some(0) {
			self.set_generation(row_sequence);
		}
	}
}

/// Metadata shared by collection types that use a generation to hide stale
/// sub-keys.
pub(crate) trait CollectionMeta: TopLevelValue {}

impl<T: TopLevelValue> TopLevelState for T {
	fn decode_state(bytes: &[u8]) -> Result<Self, DecoderError> {
		T::decode(bytes)
	}

	fn data_type(&self) -> DataType {
		T::DATA_TYPE
	}

	fn embedded_expire_time(&self) -> Option<u64> {
		T::HAS_EMBEDDED_EXPIRATION.then(|| self.expire_time())
	}

	fn set_embedded_expire_time(&mut self, timestamp: u64) {
		if T::HAS_EMBEDDED_EXPIRATION {
			self.set_expire_time(timestamp);
		}
	}

	fn resolve_pending_generation(&mut self, row_sequence: u64) {
		TopLevelValue::resolve_pending_generation(self, row_sequence);
	}
}

impl TopLevelValue for StringValue {
	const DATA_TYPE: DataType = DataType::String;
	const HAS_EMBEDDED_EXPIRATION: bool = false;

	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}

	fn expire_time(&self) -> u64 {
		0
	}

	fn set_expire_time(&mut self, _timestamp: u64) {}
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

impl TopLevelValue for HashMetaValue {
	const DATA_TYPE: DataType = DataType::Hash;

	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}

	fn expire_time(&self) -> u64 {
		self.expire_time
	}

	fn set_expire_time(&mut self, timestamp: u64) {
		self.expire_time = timestamp;
	}

	fn generation(&self) -> Option<u64> {
		Some(self.version)
	}

	fn set_generation(&mut self, generation: u64) {
		self.version = generation;
	}
}

impl CollectionMeta for HashMetaValue {}

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

impl TopLevelValue for ListMetaValue {
	const DATA_TYPE: DataType = DataType::List;

	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}

	fn expire_time(&self) -> u64 {
		self.expire_time
	}

	fn set_expire_time(&mut self, timestamp: u64) {
		self.expire_time = timestamp;
	}

	fn generation(&self) -> Option<u64> {
		Some(self.version)
	}

	fn set_generation(&mut self, generation: u64) {
		self.version = generation;
	}
}

impl CollectionMeta for ListMetaValue {}

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

impl TopLevelValue for SetMetaValue {
	const DATA_TYPE: DataType = DataType::Set;

	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}

	fn expire_time(&self) -> u64 {
		self.expire_time
	}

	fn set_expire_time(&mut self, timestamp: u64) {
		self.expire_time = timestamp;
	}

	fn generation(&self) -> Option<u64> {
		Some(self.version)
	}

	fn set_generation(&mut self, generation: u64) {
		self.version = generation;
	}
}

impl CollectionMeta for SetMetaValue {}

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

impl TopLevelValue for ZSetMetaValue {
	const DATA_TYPE: DataType = DataType::ZSet;

	fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn encode(&self) -> Bytes {
		self.encode()
	}

	fn expire_time(&self) -> u64 {
		self.expire_time
	}

	fn set_expire_time(&mut self, timestamp: u64) {
		self.expire_time = timestamp;
	}

	fn generation(&self) -> Option<u64> {
		Some(self.version)
	}

	fn set_generation(&mut self, generation: u64) {
		self.version = generation;
	}
}

impl CollectionMeta for ZSetMetaValue {}

/// A decoded top-level row used by typed keyspace lifecycle operations.
pub(crate) enum AnyValue {
	String(StringValue),
	Hash(HashMetaValue),
	List(ListMetaValue),
	Set(SetMetaValue),
	ZSet(ZSetMetaValue),
}

impl AnyValue {
	pub(crate) fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
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

	pub(crate) fn data_type(&self) -> DataType {
		match self {
			Self::String(_) => DataType::String,
			Self::Hash(_) => DataType::Hash,
			Self::List(_) => DataType::List,
			Self::Set(_) => DataType::Set,
			Self::ZSet(_) => DataType::ZSet,
		}
	}

	pub(crate) fn encode(&self) -> Bytes {
		match self {
			Self::String(v) => v.encode(),
			Self::Hash(v) => v.encode(),
			Self::List(v) => v.encode(),
			Self::Set(v) => v.encode(),
			Self::ZSet(v) => v.encode(),
		}
	}

	pub(crate) fn version(&self) -> Option<u64> {
		match self {
			Self::String(_) => None,
			Self::Hash(v) => Some(v.version),
			Self::List(v) => Some(v.version),
			Self::Set(v) => Some(v.version),
			Self::ZSet(v) => Some(v.version),
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

impl TopLevelState for AnyValue {
	fn decode_state(bytes: &[u8]) -> Result<Self, DecoderError> {
		Self::decode(bytes)
	}

	fn data_type(&self) -> DataType {
		self.data_type()
	}

	fn embedded_expire_time(&self) -> Option<u64> {
		match self {
			Self::String(_) => None,
			Self::Hash(value) => Some(value.expire_time),
			Self::List(value) => Some(value.expire_time),
			Self::Set(value) => Some(value.expire_time),
			Self::ZSet(value) => Some(value.expire_time),
		}
	}

	fn set_embedded_expire_time(&mut self, timestamp: u64) {
		self.set_expire_time(timestamp);
	}

	fn resolve_pending_generation(&mut self, row_sequence: u64) {
		if self.version() == Some(0) {
			self.set_version(row_sequence);
		}
	}
}

impl AnyValue {
	#[cfg(test)]
	pub(crate) fn expire_time(&self) -> u64 {
		self.embedded_expire_time().unwrap_or(0)
	}

	pub(crate) fn set_expire_time(&mut self, timestamp: u64) {
		match self {
			Self::String(_) => {}
			Self::Hash(v) => v.expire_time = timestamp,
			Self::List(v) => v.expire_time = timestamp,
			Self::Set(v) => v.expire_time = timestamp,
			Self::ZSet(v) => v.expire_time = timestamp,
		}
	}

	pub(crate) fn set_version(&mut self, version: u64) {
		match self {
			Self::String(_) => {}
			Self::Hash(v) => v.version = version,
			Self::List(v) => v.version = version,
			Self::Set(v) => v.version = version,
			Self::ZSet(v) => v.version = version,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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
