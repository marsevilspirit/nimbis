use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::error::DecoderError;
use crate::error::StorageError;
use crate::top_level_key::MAX_ENCODED_KEY_LEN;
use crate::top_level_key::TopLevelKey;

#[derive(Debug, PartialEq)]
pub struct MemberKey {
	user_key: Bytes,
	member: Bytes,
}

impl MemberKey {
	pub fn new(user_key: impl Into<Bytes>, member: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
			member: member.into(),
		}
	}

	pub fn encode(&self) -> Result<Bytes, StorageError> {
		// Key format: len(user_key) (u16 BE) + user_key + b'M' +
		// len(member) (u32 BE) + member
		let top_level_key = TopLevelKey::new(self.user_key.clone())?;
		let suffix_len =
			5usize
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
		suffix.put_u8(b'M');
		suffix.put_u32(member_len);
		suffix.extend_from_slice(&self.member);
		top_level_key.with_suffix(&suffix)
	}

	pub(crate) fn decode(encoded: &[u8]) -> Result<Self, DecoderError> {
		let (user_key, mut suffix) = TopLevelKey::decode_prefix(encoded)?;
		if suffix.len() < 5 || suffix.get_u8() != b'M' {
			return Err(DecoderError::InvalidLength);
		}
		let member_len = suffix.get_u32() as usize;
		if suffix.len() != member_len {
			return Err(DecoderError::InvalidLength);
		}
		Ok(Self::new(user_key, Bytes::copy_from_slice(suffix)))
	}
}
