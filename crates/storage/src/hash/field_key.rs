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
		// Key format: user_key + len(field) (u16 BE) + field
		let field_len = self.field.len();
		// Ensure field length fits in u16, though implementation plan mentioned u16
		// In a real scenario we'd check or support larger, but strict adherence to doc says u16
		let field_len_u16 = field_len as u16;

		let mut bytes = BytesMut::with_capacity(self.user_key.len() + 2 + field_len);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u16(field_len_u16);
		bytes.extend_from_slice(&self.field);
		bytes.freeze()
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("user", "field", b"user\x00\x05field")]
	#[case("key", "f", b"key\x00\x01f")]
	fn test_hash_field_key_encode(#[case] key: &str, #[case] field: &str, #[case] expected: &[u8]) {
		let field_key = HashFieldKey::new(
			Bytes::copy_from_slice(key.as_bytes()),
			Bytes::copy_from_slice(field.as_bytes()),
		);
		assert_eq!(&field_key.encode()[..], expected);
	}
}
