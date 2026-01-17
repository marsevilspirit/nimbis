use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

#[derive(Debug, PartialEq)]
pub struct MemberKey {
	user_key: Bytes,
	member: Bytes,
}

impl MemberKey {
	pub fn new(user_key: impl Into<Bytes>, member: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
			member: member.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		// Key format: len(user_key) (u16 BE) + user_key + b'M' + len(member) (u32 BE) +
		// member
		let user_key_len = self.user_key.len() as u16;
		let member_len = self.member.len() as u32;

		let mut bytes =
			BytesMut::with_capacity(2 + self.user_key.len() + 1 + 4 + self.member.len());
		bytes.put_u16(user_key_len);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u8(b'M');
		bytes.put_u32(member_len);
		bytes.extend_from_slice(&self.member);
		bytes.freeze()
	}
}
