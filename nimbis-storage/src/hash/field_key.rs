use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::error::DecoderError;
use crate::error::StorageError;
use crate::top_level_key::MAX_ENCODED_KEY_LEN;
use crate::top_level_key::TopLevelKey;

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

	pub fn encode(&self) -> Result<Bytes, StorageError> {
		// Key format: len(user_key) (u16 BE) + user_key + len(field) (u32 BE) + field
		let top_level_key = TopLevelKey::new(self.user_key.clone())?;
		let suffix_len =
			4usize
				.checked_add(self.field.len())
				.ok_or(StorageError::InvalidKeyLength {
					length: usize::MAX,
					max: MAX_ENCODED_KEY_LEN,
				})?;
		top_level_key.ensure_suffix_len(suffix_len)?;
		let field_len =
			u32::try_from(self.field.len()).map_err(|_| StorageError::InvalidKeyLength {
				length: self.field.len(),
				max: MAX_ENCODED_KEY_LEN,
			})?;

		let mut suffix = BytesMut::with_capacity(suffix_len);
		suffix.put_u32(field_len);
		suffix.extend_from_slice(&self.field);
		top_level_key.with_suffix(&suffix)
	}

	pub(crate) fn decode(encoded: &[u8]) -> Result<Self, DecoderError> {
		let (user_key, mut suffix) = TopLevelKey::decode_prefix(encoded)?;
		if suffix.len() < 4 {
			return Err(DecoderError::InvalidLength);
		}
		let field_len = suffix.get_u32() as usize;
		if suffix.len() != field_len {
			return Err(DecoderError::InvalidLength);
		}
		Ok(Self::new(user_key, Bytes::copy_from_slice(suffix)))
	}

	pub(crate) fn field(&self) -> &Bytes {
		&self.field
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
		let encoded = field_key.encode().unwrap();
		// Verify format: key_len(u16) + key + field_len(u32) + field
		assert_eq!(&encoded[..2], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[2..2 + key.len()], key.as_bytes());
	}

	#[test]
	fn rejects_a_field_that_makes_the_composite_key_too_long() {
		let field_key = HashFieldKey::new(
			Bytes::from_static(b"key"),
			Bytes::from(vec![b'f'; MAX_ENCODED_KEY_LEN]),
		);

		let error = field_key.encode().unwrap_err();
		assert!(matches!(
			error,
			StorageError::InvalidKeyLength {
				length,
				max: MAX_ENCODED_KEY_LEN
			} if length > MAX_ENCODED_KEY_LEN
		));
	}
}
