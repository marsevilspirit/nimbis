use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;
use chrono::Utc;

/// Check if a given expire_ts (milliseconds since epoch) has passed.
pub fn is_expired(expire_ts: Option<i64>) -> bool {
	expire_ts.is_some_and(|ts| ts <= Utc::now().timestamp_millis())
}

/// Build the common storage prefix: len(user_key) (u16 BE) + user_key.
pub fn user_key_prefix(key: &Bytes) -> Bytes {
	let mut prefix = BytesMut::with_capacity(2 + key.len());
	prefix.put_u16(key.len() as u16);
	prefix.extend_from_slice(key);
	prefix.freeze()
}

/// Build the current collection generation prefix:
/// len(user_key) (u16 BE) + user_key + generation (u64 BE).
pub fn collection_generation_prefix(key: &Bytes, generation: u64) -> Bytes {
	let mut prefix = BytesMut::with_capacity(2 + key.len() + 8);
	prefix.put_u16(key.len() as u16);
	prefix.extend_from_slice(key);
	prefix.put_u64(generation);
	prefix.freeze()
}

/// Build zset score-key prefix:
/// len(user_key) (u16 BE) + user_key + generation (u64 BE) + b'S'.
pub fn zset_score_user_key_prefix(key: &Bytes, generation: u64) -> Bytes {
	let mut prefix = BytesMut::with_capacity(2 + key.len() + 8 + 1);
	prefix.put_u16(key.len() as u16);
	prefix.extend_from_slice(key);
	prefix.put_u64(generation);
	prefix.put_u8(b'S');
	prefix.freeze()
}

/// Decode the common collection sub-key header:
/// len(user_key) (u16 BE) + user_key + generation (u64 BE) + ...
pub fn decode_collection_generation(payload: &[u8]) -> Option<(Bytes, u64)> {
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
	let generation = buf.get_u64();
	Some((user_key, generation))
}
