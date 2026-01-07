use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

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

	pub fn encode(&self) -> Bytes {
		// Key format: user_key + b'S' + score (u64 big endian, bit flipped) + member
		// We use a custom encoding for f64 to ensure correct sorting order.
		// IEEE 754 floats don't sort correctly when treated as bytes (especially negative numbers).
		// A common trick is to flip the sign bit if positive, or flip all bits if negative.
		// However, for simplicity and standard practice in key-value stores (like CockroachDB or others):
		// If sign bit is 0 (positive): flip sign bit (becomes 1)
		// If sign bit is 1 (negative): flip all bits
		// This maps:
		// -0.0 -> 0x8000...
		// +0.0 -> 0x8000...
		// Negative numbers -> 0x00... to 0x7F... (ascending)
		// Positive numbers -> 0x80... to 0xFF... (ascending)

		let bits = self.score.to_bits();
		let encoded_score = if self.score >= 0.0 {
			bits | 0x8000_0000_0000_0000
		} else {
			!bits
		};

		let user_key_len = self.user_key.len() as u16;

		let mut bytes =
			BytesMut::with_capacity(2 + self.user_key.len() + 1 + 8 + self.member.len());
		bytes.put_u16(user_key_len);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u8(b'S');
		bytes.put_u64(encoded_score);
		bytes.extend_from_slice(&self.member);
		bytes.freeze()
	}

	pub fn decode_score(encoded: u64) -> f64 {
		let bits = if (encoded & 0x8000_0000_0000_0000) != 0 {
			encoded & !0x8000_0000_0000_0000
		} else {
			!encoded
		};
		f64::from_bits(bits)
	}
}
