use std::ops::Bound;

use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::error::DecoderError;
use crate::error::StorageError;

pub(crate) const MAX_ENCODED_KEY_LEN: usize = u16::MAX as usize;
pub(crate) const MAX_USER_KEY_LEN: usize = MAX_ENCODED_KEY_LEN - 2;

/// A validated user key shared by top-level rows and collection sub-keys.
///
/// The on-disk layout starts every row with a big-endian `u16` user-key
/// length. Keeping validation and encoding here prevents the metadata,
/// collection and compaction paths from implementing that contract
/// independently.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TopLevelKey {
	user_key: Bytes,
}

impl TopLevelKey {
	pub(crate) fn new(user_key: impl Into<Bytes>) -> Result<Self, StorageError> {
		let user_key = user_key.into();
		if user_key.len() > MAX_USER_KEY_LEN {
			return Err(StorageError::InvalidKeyLength {
				length: user_key.len(),
				max: MAX_USER_KEY_LEN,
			});
		}
		Ok(Self { user_key })
	}

	pub(crate) fn decode_exact(encoded: &[u8]) -> Result<Self, DecoderError> {
		let (user_key, suffix) = Self::decode_prefix(encoded)?;
		if !suffix.is_empty() {
			return Err(DecoderError::InvalidLength);
		}
		Ok(Self { user_key })
	}

	pub(crate) fn decode_prefix(encoded: &[u8]) -> Result<(Bytes, &[u8]), DecoderError> {
		if encoded.len() < 2 {
			return Err(DecoderError::InvalidLength);
		}
		let mut remaining = encoded;
		let key_len = remaining.get_u16() as usize;
		if key_len > MAX_USER_KEY_LEN || remaining.len() < key_len {
			return Err(DecoderError::InvalidLength);
		}
		let (user_key, suffix) = remaining.split_at(key_len);
		Ok((Bytes::copy_from_slice(user_key), suffix))
	}

	pub(crate) fn encode(&self) -> Bytes {
		let mut encoded = BytesMut::with_capacity(2 + self.user_key.len());
		encoded.put_u16(self.user_key.len() as u16);
		encoded.extend_from_slice(&self.user_key);
		encoded.freeze()
	}

	pub(crate) fn sub_key_range(&self) -> Result<(Bound<Bytes>, Bound<Bytes>), StorageError> {
		let prefix = self.encode();
		ensure_encoded_key_len(prefix.len() + 1)?;
		let mut start = BytesMut::with_capacity(prefix.len() + 1);
		start.extend_from_slice(&prefix);
		start.put_u8(0);

		let mut upper = prefix.to_vec();
		let end = upper
			.iter()
			.rposition(|byte| *byte != u8::MAX)
			.map(|index| {
				upper[index] += 1;
				upper.truncate(index + 1);
				Bound::Excluded(Bytes::from(upper))
			})
			.unwrap_or(Bound::Unbounded);

		Ok((Bound::Included(start.freeze()), end))
	}

	pub(crate) fn with_suffix(&self, suffix: &[u8]) -> Result<Bytes, StorageError> {
		self.ensure_suffix_len(suffix.len())?;
		let prefix = self.encode();
		let mut encoded = BytesMut::with_capacity(prefix.len() + suffix.len());
		encoded.extend_from_slice(&prefix);
		encoded.extend_from_slice(suffix);
		Ok(encoded.freeze())
	}

	pub(crate) fn ensure_suffix_len(&self, suffix_len: usize) -> Result<(), StorageError> {
		let encoded_len = 2usize
			.checked_add(self.user_key.len())
			.and_then(|prefix_len| prefix_len.checked_add(suffix_len))
			.ok_or(StorageError::InvalidKeyLength {
				length: usize::MAX,
				max: MAX_ENCODED_KEY_LEN,
			})?;
		ensure_encoded_key_len(encoded_len)
	}
}

pub(crate) fn ensure_encoded_key_len(length: usize) -> Result<(), StorageError> {
	if length > MAX_ENCODED_KEY_LEN {
		return Err(StorageError::InvalidKeyLength {
			length,
			max: MAX_ENCODED_KEY_LEN,
		});
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use std::ops::RangeBounds;

	use super::*;

	#[test]
	fn exact_codec_round_trips_the_maximum_key() {
		let key = TopLevelKey::new(Bytes::from(vec![b'k'; MAX_USER_KEY_LEN])).unwrap();
		let decoded = TopLevelKey::decode_exact(&key.encode()).unwrap();
		assert_eq!(decoded, key);
	}

	#[test]
	fn rejects_unrepresentable_keys() {
		let error = TopLevelKey::new(Bytes::from(vec![b'k'; MAX_USER_KEY_LEN + 1])).unwrap_err();
		assert!(matches!(
			error,
			StorageError::InvalidKeyLength {
				length,
				max: MAX_USER_KEY_LEN
			} if length == MAX_USER_KEY_LEN + 1
		));
	}

	#[test]
	fn decoder_rejects_an_oversized_user_key_prefix() {
		let oversized_len = MAX_USER_KEY_LEN + 1;
		let mut encoded = BytesMut::with_capacity(2 + oversized_len);
		encoded.put_u16(oversized_len as u16);
		encoded.resize(2 + oversized_len, b'k');
		assert!(matches!(
			TopLevelKey::decode_exact(&encoded),
			Err(DecoderError::InvalidLength)
		));
	}

	#[test]
	fn sub_key_range_excludes_metadata_and_neighboring_keys() {
		let key = TopLevelKey::new(Bytes::from_static(b"hash")).unwrap();
		let encoded = key.encode();
		let range = key.sub_key_range().unwrap();
		assert!(!range.contains(&encoded));
		assert!(range.contains(&key.with_suffix(&[0]).unwrap()));

		let neighbor = TopLevelKey::new(Bytes::from_static(b"hash:neighbor"))
			.unwrap()
			.encode();
		assert!(!range.contains(&neighbor));
	}
}
