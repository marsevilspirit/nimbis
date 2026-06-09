use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::segment::LIST_PREFIX;

#[derive(Debug, PartialEq)]
pub struct ListElementKey {
	user_key: Bytes,
	version: u64,
	seq: u64,
}

impl ListElementKey {
	pub fn new(user_key: impl Into<Bytes>, version: u64, seq: u64) -> Self {
		Self {
			user_key: user_key.into(),
			version,
			seq,
		}
	}

	pub fn encode(&self) -> Bytes {
		// Key format: b'l' + len(user_key) (u16 BE) + user_key + version (u64 BE)
		// + seq (u64 BE)
		let mut bytes = BytesMut::with_capacity(1 + 2 + self.user_key.len() + 8 + 8);
		bytes.put_u8(LIST_PREFIX);
		bytes.put_u16(self.user_key.len() as u16);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u64(self.version);
		bytes.put_u64(self.seq);
		bytes.freeze()
	}

	/// Returns the user_key from this element key.
	pub fn user_key(&self) -> &Bytes {
		&self.user_key
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
		let version = 0x0102_0304_0506_0708;
		let element_key = ListElementKey::new(Bytes::copy_from_slice(key.as_bytes()), version, seq);
		let encoded = element_key.encode();
		// Verify format: b'l' + key_len(u16) + key + version(u64) + seq(u64)
		assert_eq!(encoded[0], b'l');
		assert_eq!(&encoded[1..3], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[3..3 + key.len()], key.as_bytes());
		let version_start = 3 + key.len();
		assert_eq!(
			&encoded[version_start..version_start + 8],
			&version.to_be_bytes()
		);
		assert_eq!(
			&encoded[version_start + 8..version_start + 16],
			&seq.to_be_bytes()
		);
	}
}
