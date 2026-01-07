use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

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

	pub fn encode(&self) -> Bytes {
		// Key format: len(user_key) (u16 BE) + user_key + seq (u64 BE)
		let mut bytes = BytesMut::with_capacity(2 + self.user_key.len() + 8);
		bytes.put_u16(self.user_key.len() as u16);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u64(self.seq);
		bytes.freeze()
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("mykey", 1, b"\x00\x05mykey\x00\x00\x00\x00\x00\x00\x00\x01")]
	#[case("key", 255, b"\x00\x03key\x00\x00\x00\x00\x00\x00\x00\xff")]
	fn test_list_element_key_encode(#[case] key: &str, #[case] seq: u64, #[case] expected: &[u8]) {
		let element_key = ListElementKey::new(Bytes::copy_from_slice(key.as_bytes()), seq);
		assert_eq!(&element_key.encode()[..], expected);
	}
}
