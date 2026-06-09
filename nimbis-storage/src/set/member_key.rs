use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::segment::SET_PREFIX;

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
		// Key format:
		// b'S' + len(user_key) (u16 BE) + user_key + version (u64 BE) +
		// len(member) (u32 BE) + member
		let member_len = self.member.len() as u32;

		let mut bytes =
			BytesMut::with_capacity(1 + 2 + self.user_key.len() + 8 + 4 + self.member.len());
		bytes.put_u8(SET_PREFIX);
		bytes.put_u16(self.user_key.len() as u16);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u64(self.version);
		bytes.put_u32(member_len);
		bytes.extend_from_slice(&self.member);
		bytes.freeze()
	}

	pub fn prefix(user_key: &Bytes, version: u64) -> Bytes {
		let mut bytes = BytesMut::with_capacity(1 + 2 + user_key.len() + 8);
		bytes.put_u8(SET_PREFIX);
		bytes.put_u16(user_key.len() as u16);
		bytes.extend_from_slice(user_key);
		bytes.put_u64(version);
		bytes.freeze()
	}

	pub fn user_prefix(user_key: &Bytes) -> Bytes {
		let mut bytes = BytesMut::with_capacity(1 + 2 + user_key.len());
		bytes.put_u8(SET_PREFIX);
		bytes.put_u16(user_key.len() as u16);
		bytes.extend_from_slice(user_key);
		bytes.freeze()
	}

	/// Returns the user_key from this member key.
	pub fn user_key(&self) -> &Bytes {
		&self.user_key
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("user", "member")]
	#[case("key", "m")]
	fn test_set_member_key_encode(#[case] key: &str, #[case] member: &str) {
		let version = 0x0102_0304_0506_0708;
		let member_key = SetMemberKey::new(
			Bytes::copy_from_slice(key.as_bytes()),
			version,
			Bytes::copy_from_slice(member.as_bytes()),
		);
		let encoded = member_key.encode();
		// Verify format: b'S' + key_len(u16) + key + version(u64) +
		// member_len(u32) + member
		assert_eq!(encoded[0], b'S');
		assert_eq!(&encoded[1..3], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[3..3 + key.len()], key.as_bytes());
		let version_start = 3 + key.len();
		assert_eq!(
			&encoded[version_start..version_start + 8],
			&version.to_be_bytes()
		);
	}
}
