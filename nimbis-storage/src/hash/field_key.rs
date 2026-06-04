use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::segment::HASH_PREFIX;

#[derive(Debug, PartialEq)]
pub struct HashFieldKey {
	user_key: Bytes,
	version: u64,
	field: Bytes,
}

impl HashFieldKey {
	pub fn new(user_key: impl Into<Bytes>, version: u64, field: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
			version,
			field: field.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		// Key format:
		// b'h' + len(user_key) (u16 BE) + user_key + version (u64 BE) +
		// len(field) (u32 BE) + field
		let field_len = self.field.len() as u32;

		let mut bytes =
			BytesMut::with_capacity(1 + 2 + self.user_key.len() + 8 + 4 + self.field.len());
		bytes.put_u8(HASH_PREFIX);
		bytes.put_u16(self.user_key.len() as u16);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u64(self.version);
		bytes.put_u32(field_len);
		bytes.extend_from_slice(&self.field);
		bytes.freeze()
	}

	pub fn prefix(user_key: &Bytes, version: u64) -> Bytes {
		let mut bytes = BytesMut::with_capacity(1 + 2 + user_key.len() + 8);
		bytes.put_u8(HASH_PREFIX);
		bytes.put_u16(user_key.len() as u16);
		bytes.extend_from_slice(user_key);
		bytes.put_u64(version);
		bytes.freeze()
	}

	/// Returns the user_key from this field key.
	pub fn user_key(&self) -> &Bytes {
		&self.user_key
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("user", "field")]
	#[case("key", "f")]
	fn test_hash_field_key_encode(#[case] key: &str, #[case] field: &str) {
		let version = 0x0102_0304_0506_0708;
		let field_key = HashFieldKey::new(
			Bytes::copy_from_slice(key.as_bytes()),
			version,
			Bytes::copy_from_slice(field.as_bytes()),
		);
		let encoded = field_key.encode();
		// Verify format: b'h' + key_len(u16) + key + version(u64) +
		// field_len(u32) + field
		assert_eq!(encoded[0], b'h');
		assert_eq!(&encoded[1..3], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[3..3 + key.len()], key.as_bytes());
		let version_start = 3 + key.len();
		assert_eq!(
			&encoded[version_start..version_start + 8],
			&version.to_be_bytes()
		);
	}
}
