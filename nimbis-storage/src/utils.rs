use bytes::Buf;
use bytes::Bytes;
use chrono::Utc;

/// Check if a given expire_ts (milliseconds since epoch) has passed.
pub fn is_expired(expire_ts: Option<i64>) -> bool {
	expire_ts.is_some_and(|ts| ts <= Utc::now().timestamp_millis())
}

/// Decode the common collection sub-key header:
/// len(user_key) (u16 BE) + user_key + version (u64 BE) + ...
pub fn decode_collection_version(payload: &[u8]) -> Option<(Bytes, u64)> {
	if payload.len() < 2 {
		return None;
	}
	let mut buf = payload;
	let key_len = buf.get_u16() as usize;
	if buf.len() < key_len + 8 {
		return None;
	}
	let user_key = Bytes::copy_from_slice(&buf[..key_len]);
	buf.advance(key_len);
	let version = buf.get_u64();
	Some((user_key, version))
}
