use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

#[derive(Debug, PartialEq)]
pub struct MemberKey {
	user_key: Bytes,
	version: u64,
	member: Bytes,
}

impl MemberKey {
	pub fn new(user_key: impl Into<Bytes>, version: u64, member: impl Into<Bytes>) -> Self {
		Self {
			user_key: user_key.into(),
			version,
			member: member.into(),
		}
	}

	pub fn encode(&self) -> Bytes {
		// Key format: len(user_key) (u16 BE) + user_key + version (u64 BE) + b'M' +
		// len(member) (u32 BE) + member
		let user_key_len = self.user_key.len() as u16;
		let member_len = self.member.len() as u32;

		let mut bytes =
			BytesMut::with_capacity(2 + self.user_key.len() + 8 + 1 + 4 + self.member.len());
		bytes.put_u16(user_key_len);
		bytes.extend_from_slice(&self.user_key);
		bytes.put_u64(self.version);
		bytes.put_u8(b'M');
		bytes.put_u32(member_len);
		bytes.extend_from_slice(&self.member);
		bytes.freeze()
	}

	/// Returns the user_key from this member key.
	pub fn user_key(&self) -> &Bytes {
		&self.user_key
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_zset_member_key_encode_includes_version() {
		let version = 0x0102_0304_0506_0708;
		let key = Bytes::from("myzset");
		let member = Bytes::from("member");
		let encoded = MemberKey::new(key.clone(), version, member).encode();
		let version_start = 2 + key.len();

		assert_eq!(&encoded[..2], &(key.len() as u16).to_be_bytes());
		assert_eq!(&encoded[2..2 + key.len()], key.as_ref());
		assert_eq!(
			&encoded[version_start..version_start + 8],
			&version.to_be_bytes()
		);
		assert_eq!(encoded[version_start + 8], b'M');
	}
}
