use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

#[derive(Debug, PartialEq)]
pub struct HashFieldKey {
	user_key: Bytes,
	field: Bytes,
}

impl HashFieldKey {
	pub fn new(user_key: impl Into<Bytes>, field: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
			field: field.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		// Key format: len(user_key) (u16 BE) + user_key + len(field) (u32 BE) + field
		let field_len = self.field.len() as u32;

		let mut bytes = BytesMut::with_capacity(2 + self.user_key.len() + 4 + self.field.len());
		bytes.put_u16(self.user_key.len() as u16);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u32(field_len);
		bytes.extend_from_slice(&self.field);
		bytes.freeze()
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("user", "field", b"\x00\x04user\x00\x00\x00\x05field")]
	#[case("key", "f", b"\x00\x03key\x00\x00\x00\x01f")]
	fn test_hash_field_key_encode(#[case] key: &str, #[case] field: &str, #[case] expected: &[u8]) {
		let field_key = HashFieldKey::new(
			Bytes::copy_from_slice(key.as_bytes()),
			Bytes::copy_from_slice(field.as_bytes()),
		);
		assert_eq!(&field_key.encode()[..], expected);
	}
}
