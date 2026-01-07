use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

#[derive(Debug, PartialEq)]
pub struct SetMemberKey {
	user_key: Bytes,
	member: Bytes,
}

impl SetMemberKey {
	pub fn new(user_key: impl Into<Bytes>, member: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
			member: member.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		// Key format: len(user_key) (u16 BE) + user_key + len(member) (u32 BE) + member
		let member_len = self.member.len() as u32;

		let mut bytes = BytesMut::with_capacity(2 + self.user_key.len() + 4 + self.member.len());
		bytes.put_u16(self.user_key.len() as u16);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u32(member_len);
		bytes.extend_from_slice(&self.member);
		bytes.freeze()
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("user", "member", b"\x00\x04user\x00\x00\x00\x06member")]
	#[case("key", "m", b"\x00\x03key\x00\x00\x00\x01m")]
	fn test_set_member_key_encode(
		#[case] key: &str,
		#[case] member: &str,
		#[case] expected: &[u8],
	) {
		let member_key = SetMemberKey::new(
			Bytes::copy_from_slice(key.as_bytes()),
			Bytes::copy_from_slice(member.as_bytes()),
		);
		assert_eq!(&member_key.encode()[..], expected);
	}
}
