use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::error::DecoderError;
use crate::error::StorageError;
use crate::top_level_key::MAX_ENCODED_KEY_LEN;
use crate::top_level_key::TopLevelKey;

#[derive(Debug, PartialEq)]
pub struct SetMemberKey {
	user_key: Bytes,
	member: Bytes,
}

impl SetMemberKey {
	pub fn new(user_key: impl Into<Bytes>, member: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
			member: member.into(),
		}
	}

	pub fn encode(&self) -> Result<Bytes, StorageError> {
		// Key format: len(user_key) (u16 BE) + user_key + len(member) (u32 BE) + member
		let top_level_key = TopLevelKey::new(self.user_key.clone())?;
		let suffix_len =
			4usize
				.checked_add(self.member.len())
				.ok_or(StorageError::InvalidKeyLength {
					length: usize::MAX,
					max: MAX_ENCODED_KEY_LEN,
				})?;
		top_level_key.ensure_suffix_len(suffix_len)?;
		let member_len =
			u32::try_from(self.member.len()).map_err(|_| StorageError::InvalidKeyLength {
				length: self.member.len(),
				max: MAX_ENCODED_KEY_LEN,
			})?;

		let mut suffix = BytesMut::with_capacity(suffix_len);
		suffix.put_u32(member_len);
		suffix.extend_from_slice(&self.member);
		top_level_key.with_suffix(&suffix)
	}

	pub(crate) fn decode(encoded: &[u8]) -> Result<Self, DecoderError> {
		let (user_key, mut suffix) = TopLevelKey::decode_prefix(encoded)?;
		if suffix.len() < 4 {
			return Err(DecoderError::InvalidLength);
		}
		let member_len = suffix.get_u32() as usize;
		if suffix.len() != member_len {
			return Err(DecoderError::InvalidLength);
		}
		Ok(Self::new(user_key, Bytes::copy_from_slice(suffix)))
	}

	pub(crate) fn member(&self) -> &Bytes {
		&self.member
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("user", "member")]
	#[case("key", "m")]
	fn test_set_member_key_encode(#[case] key: &str, #[case] member: &str) {
		let member_key = SetMemberKey::new(
			Bytes::copy_from_slice(key.as_bytes()),
			Bytes::copy_from_slice(member.as_bytes()),
		);
		let encoded = member_key.encode().unwrap();
		// Verify format: key_len(u16) + key + member_len(u32) + member
		assert_eq!(&encoded[..2], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[2..2 + key.len()], key.as_bytes());
	}
}
