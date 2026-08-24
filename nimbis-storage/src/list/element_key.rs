use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::error::DecoderError;
use crate::error::StorageError;
use crate::top_level_key::TopLevelKey;

#[derive(Debug, PartialEq)]
pub struct ListElementKey {
	user_key: Bytes,
	seq: u64,
}

impl ListElementKey {
	pub fn new(user_key: impl Into<Bytes>, seq: u64) -> Self {
		Self {
			user_key: user_key.into(),
			seq,
		}
	}

	pub fn encode(&self) -> Result<Bytes, StorageError> {
		// Key format: len(user_key) (u16 BE) + user_key + seq (u64 BE)
		let top_level_key = TopLevelKey::new(self.user_key.clone())?;
		top_level_key.ensure_suffix_len(8)?;
		let mut suffix = BytesMut::with_capacity(8);
		suffix.put_u64(self.seq);
		top_level_key.with_suffix(&suffix)
	}

	pub(crate) fn decode(encoded: &[u8]) -> Result<Self, DecoderError> {
		let (user_key, mut suffix) = TopLevelKey::decode_prefix(encoded)?;
		if suffix.len() != 8 {
			return Err(DecoderError::InvalidLength);
		}
		Ok(Self::new(user_key, suffix.get_u64()))
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("mykey", 100u64)]
	#[case("key", 255u64)]
	fn test_list_element_key_encode(#[case] key: &str, #[case] seq: u64) {
		let element_key = ListElementKey::new(Bytes::copy_from_slice(key.as_bytes()), seq);
		let encoded = element_key.encode().unwrap();
		// Verify format: key_len(u16) + key + seq(u64)
		assert_eq!(&encoded[..2], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[2..2 + key.len()], key.as_bytes());
		assert_eq!(
			&encoded[2 + key.len()..2 + key.len() + 8],
			&seq.to_be_bytes()
		);
	}
}
