use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::segment::ZSET_PREFIX;

#[derive(Debug, PartialEq)]
pub struct ScoreKey {
	user_key: Bytes,
	version: u64,
	score: f64,
	member: Bytes,
}

impl ScoreKey {
	pub fn new(
		user_key: impl Into<Bytes>,
		version: u64,
		score: f64,
		member: impl Into<Bytes>,
	) -> Self {
		Self {
			user_key: user_key.into(),
			version,
			score,
			member: member.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		// Key format: b'z' + len(user_key) (u16 BE) + user_key + version (u64 BE) +
		// b'S' + score (u64 big endian, bit flipped) + member We use a custom encoding
		// for f64 to ensure correct sorting order. IEEE 754 floats don't sort correctly
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

		let user_key_len = self.user_key.len() as u16;

		let mut bytes =
			BytesMut::with_capacity(1 + 2 + self.user_key.len() + 8 + 1 + 8 + self.member.len());
		bytes.put_u8(ZSET_PREFIX);
		bytes.put_u16(user_key_len);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u64(self.version);
		bytes.put_u8(b'S');
		bytes.put_u64(encoded_score);
		bytes.extend_from_slice(&self.member);
		bytes.freeze()
	}

	pub fn prefix(user_key: &Bytes, version: u64) -> Bytes {
		let mut bytes = BytesMut::with_capacity(1 + 2 + user_key.len() + 8 + 1);
		bytes.put_u8(ZSET_PREFIX);
		bytes.put_u16(user_key.len() as u16);
		bytes.extend_from_slice(user_key);
		bytes.put_u64(version);
		bytes.put_u8(b'S');
		bytes.freeze()
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

	/// Returns the user_key from this score key.
	pub fn user_key(&self) -> &Bytes {
		&self.user_key
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

	#[test]
	fn test_zset_score_key_encode_includes_version() {
		let version = 0x0102_0304_0506_0708;
		let key = Bytes::from("myzset");
		let encoded = ScoreKey::new(key.clone(), version, 1.5, Bytes::from("member")).encode();
		let version_start = 3 + key.len();

		assert_eq!(encoded[0], b'z');
		assert_eq!(&encoded[1..3], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[3..3 + key.len()], key.as_ref());
		assert_eq!(
			&encoded[version_start..version_start + 8],
			&version.to_be_bytes()
		);
		assert_eq!(encoded[version_start + 8], b'S');
	}
}
