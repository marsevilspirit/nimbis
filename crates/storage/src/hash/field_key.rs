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
		let field_key = HashFieldKey::new(
			Bytes::copy_from_slice(key.as_bytes()),
			Bytes::copy_from_slice(field.as_bytes()),
		);
		let encoded = field_key.encode();
		// Verify format: key_len(u16) + key + field_len(u32) + field
		assert_eq!(&encoded[..2], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[2..2 + key.len()], key.as_bytes());
	}
}
