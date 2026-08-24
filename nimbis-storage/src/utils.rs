use std::ops::Bound;

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

/// Build a half-open range that contains collection sub-keys but excludes the
/// exact metadata key. Every collection sub-key has at least one suffix byte,
/// so `prefix + 0x00` is the smallest possible sub-key.
pub fn user_key_sub_key_range(key: &Bytes) -> (Bound<Bytes>, Bound<Bytes>) {
	let prefix = user_key_prefix(key);
	let mut start = BytesMut::with_capacity(prefix.len() + 1);
	start.extend_from_slice(&prefix);
	start.put_u8(0);

	let mut upper = prefix.to_vec();
	let end = upper
		.iter()
		.rposition(|byte| *byte != u8::MAX)
		.map(|index| {
			upper[index] += 1;
			upper.truncate(index + 1);
			Bound::Excluded(Bytes::from(upper))
		})
		.unwrap_or(Bound::Unbounded);

	(Bound::Included(start.freeze()), end)
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

#[cfg(test)]
mod tests {
	use std::ops::RangeBounds;

	use super::*;

	#[test]
	fn sub_key_range_excludes_metadata_and_neighboring_keys() {
		let key = Bytes::from_static(b"hash");
		let prefix = user_key_prefix(&key);
		let range = user_key_sub_key_range(&key);
		assert!(!range.contains(&prefix));

		let mut empty_field = BytesMut::from(prefix.as_ref());
		empty_field.put_u32(0);
		assert!(range.contains(&empty_field.freeze()));

		let neighbor = user_key_prefix(&Bytes::from_static(b"hash:neighbor"));
		assert!(!range.contains(&neighbor));
	}
}
