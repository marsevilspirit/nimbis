use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::error::DecoderError;
use crate::error::StorageError;
use crate::top_level_key::MAX_ENCODED_KEY_LEN;
use crate::top_level_key::TopLevelKey;

#[derive(Debug, PartialEq)]
pub struct ScoreKey {
	user_key: Bytes,
	score: f64,
	member: Bytes,
}

impl ScoreKey {
	pub fn new(user_key: impl Into<Bytes>, score: f64, member: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
			score,
			member: member.into(),
		}
	}

	pub fn encode(&self) -> Result<Bytes, StorageError> {
		// Key format: len(user_key) (u16 BE) + user_key + b'S' +
		// score (u64 big endian, bit flipped) + member We use a custom encoding for
		// f64 to ensure correct sorting order. IEEE 754 floats don't sort correctly
		// when treated as bytes (especially negative numbers). A common trick is to
		// flip the sign bit if positive, or flip all bits if negative. However, for
		// simplicity and standard practice in key-value stores (like CockroachDB or
		// others): If sign bit is 0 (positive): flip sign bit (becomes 1) If sign bit
		// is 1 (negative): flip all bits This maps:
		// -0.0 -> 0x8000...
		// +0.0 -> 0x8000...
		// Negative numbers -> 0x00... to 0x7F... (ascending)
		// Positive numbers -> 0x80... to 0xFF... (ascending)

		let encoded_score = Self::encode_score(self.score);

		let top_level_key = TopLevelKey::new(self.user_key.clone())?;
		let suffix_len =
			9usize
				.checked_add(self.member.len())
				.ok_or(StorageError::InvalidKeyLength {
					length: usize::MAX,
					max: MAX_ENCODED_KEY_LEN,
				})?;
		top_level_key.ensure_suffix_len(suffix_len)?;

		let mut suffix = BytesMut::with_capacity(suffix_len);
		suffix.put_u8(b'S');
		suffix.put_u64(encoded_score);
		suffix.extend_from_slice(&self.member);
		top_level_key.with_suffix(&suffix)
	}

	pub(crate) fn decode(encoded: &[u8]) -> Result<Self, DecoderError> {
		let (user_key, mut suffix) = TopLevelKey::decode_prefix(encoded)?;
		if suffix.len() < 9 || suffix.get_u8() != b'S' {
			return Err(DecoderError::InvalidLength);
		}
		let score = Self::decode_score(suffix.get_u64());
		Ok(Self::new(user_key, score, Bytes::copy_from_slice(suffix)))
	}

	/// Encode an f64 score into a u64 for byte-sortable storage.
	/// IEEE 754 floats don't sort correctly when treated as bytes (especially
	/// negative numbers). This flips bits to ensure correct byte-level
	/// ordering:
	/// - Positive numbers: set sign bit to 1
	/// - Negative numbers: flip all bits
	pub fn encode_score(score: f64) -> u64 {
		let bits = score.to_bits();
		if score >= 0.0 {
			bits | 0x8000_0000_0000_0000
		} else {
			!bits
		}
	}

	/// Decode a u64 back into an f64 score.
	pub fn decode_score(encoded: u64) -> f64 {
		let bits = if (encoded & 0x8000_0000_0000_0000) != 0 {
			encoded & !0x8000_0000_0000_0000
		} else {
			!encoded
		};
		f64::from_bits(bits)
	}

	pub(crate) fn score(&self) -> f64 {
		self.score
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
	#[case(f64::NEG_INFINITY)]
	#[case(-1e308)]
	#[case(-1000.0)]
	#[case(-1.0)]
	#[case(-0.1)]
	#[case(-0.0)]
	#[case(0.0)]
	#[case(0.1)]
	#[case(1.0)]
	#[case(1000.0)]
	#[case(1e308)]
	#[case(f64::INFINITY)]
	#[case(f64::MIN)]
	#[case(f64::MAX)]
	fn test_encode_decode_roundtrip(#[case] score: f64) {
		let encoded = ScoreKey::encode_score(score);
		let decoded = ScoreKey::decode_score(encoded);
		assert_eq!(score, decoded);
	}

	#[test]
	fn test_byte_sortable_order() {
		// Verify encoded values maintain correct ascending order
		let scores = vec![
			f64::NEG_INFINITY,
			-1000.0,
			-100.0,
			-1.0,
			-0.5,
			0.0,
			0.5,
			1.0,
			100.0,
			1000.0,
			f64::INFINITY,
		];

		let encoded: Vec<u64> = scores.iter().map(|&s| ScoreKey::encode_score(s)).collect();

		for i in 1..encoded.len() {
			assert!(
				encoded[i - 1] < encoded[i],
				"Order broken: {} ({}) >= {} ({})",
				scores[i - 1],
				encoded[i - 1],
				scores[i],
				encoded[i]
			);
		}
	}

	#[rstest]
	#[case(0.0)]
	#[case(1.0)]
	#[case(100.0)]
	#[case(f64::MAX)]
	#[case(f64::INFINITY)]
	fn test_positive_has_msb_set(#[case] score: f64) {
		let encoded = ScoreKey::encode_score(score);
		assert_ne!(encoded & 0x8000_0000_0000_0000, 0);
	}

	#[rstest]
	#[case(-1.0)]
	#[case(-100.0)]
	#[case(f64::MIN)]
	#[case(f64::NEG_INFINITY)]
	fn test_negative_has_msb_unset(#[case] score: f64) {
		let encoded = ScoreKey::encode_score(score);
		assert_eq!(encoded & 0x8000_0000_0000_0000, 0);
	}
}
