use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

#[derive(Debug, PartialEq)]
pub struct SetMemberKey {
	user_key: Bytes,
	version: u64,
	member: Bytes,
}

impl SetMemberKey {
	pub fn new(user_key: impl Into<Bytes>, version: u64, member: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
			version,
			member: member.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		// Key format: len(user_key) (u16 BE) + user_key + version (u64 BE) +
		// len(member) (u32 BE) + member
		let member_len = self.member.len() as u32;

		let mut bytes =
			BytesMut::with_capacity(2 + self.user_key.len() + 8 + 4 + self.member.len());
		bytes.put_u16(self.user_key.len() as u16);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u64(self.version);
		bytes.put_u32(member_len);
		bytes.extend_from_slice(&self.member);
		bytes.freeze()
	}

	/// Returns the user_key from this member key.
	pub fn user_key(&self) -> &Bytes {
		&self.user_key
	}

	/// Returns the version from this member key.
	pub fn version(&self) -> u64 {
		self.version
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("user", 1u64, "member")]
	#[case("key", 0u64, "m")]
	fn test_set_member_key_encode(#[case] key: &str, #[case] version: u64, #[case] member: &str) {
		let member_key = SetMemberKey::new(
			Bytes::copy_from_slice(key.as_bytes()),
			version,
			Bytes::copy_from_slice(member.as_bytes()),
		);
		let encoded = member_key.encode();
		// Verify format: key_len(u16) + key + version(u64) + member_len(u32) + member
		assert_eq!(&encoded[..2], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[2..2 + key.len()], key.as_bytes());
		let version_start = 2 + key.len();
		assert_eq!(
			&encoded[version_start..version_start + 8],
			&version.to_be_bytes()
		);
	}
}
