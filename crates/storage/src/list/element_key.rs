use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

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
		// Key format: len(user_key) (u16 BE) + user_key + version (u64 BE) + seq (u64
		// BE)
		let mut bytes = BytesMut::with_capacity(2 + self.user_key.len() + 8 + 8);
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

	/// Returns the version from this element key.
	pub fn version(&self) -> u64 {
		self.version
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("mykey", 1u64, 100u64)]
	#[case("key", 0u64, 255u64)]
	fn test_list_element_key_encode(#[case] key: &str, #[case] version: u64, #[case] seq: u64) {
		let element_key = ListElementKey::new(Bytes::copy_from_slice(key.as_bytes()), version, seq);
		let encoded = element_key.encode();
		// Verify format: key_len(u16) + key + version(u64) + seq(u64)
		assert_eq!(&encoded[..2], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[2..2 + key.len()], key.as_bytes());
		let version_start = 2 + key.len();
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
