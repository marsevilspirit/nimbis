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

/// Build zset score-key prefix:
/// len(user_key) (u16 BE) + user_key + b'S'.
pub fn zset_score_user_key_prefix(key: &Bytes) -> Bytes {
	let mut prefix = BytesMut::with_capacity(2 + key.len() + 1);
	prefix.put_u16(key.len() as u16);
	prefix.extend_from_slice(key);
	prefix.put_u8(b'S');
	prefix.freeze()
}
