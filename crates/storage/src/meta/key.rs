use bytes::Bytes;

#[derive(Debug, PartialEq)]
pub struct MetaKey {
	user_key: Bytes,
}

impl MetaKey {
	pub fn new(user_key: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		self.user_key.clone()
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("mykey", b"mykey")]
	#[case("", b"")]
	fn test_meta_key_encode(#[case] key: &str, #[case] expected: &[u8]) {
		let meta_key = MetaKey::new(Bytes::copy_from_slice(key.as_bytes()));
		assert_eq!(&meta_key.encode()[..], expected);
	}
}
