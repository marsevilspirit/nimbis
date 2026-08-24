use async_trait::async_trait;
use bytes::Buf;
use bytes::Bytes;
use log::debug;
use log::warn;
use slatedb::CompactionFilter;
use slatedb::CompactionFilterDecision;
use slatedb::CompactionFilterError;
use slatedb::CompactionFilterSupplier;
use slatedb::CompactionJobContext;
use slatedb::RowEntry;
use slatedb::ValueDeletable;

use crate::data_type::DataType;
use crate::string::meta::AnyValue;
use crate::string::meta::MetaKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cutoff {
	seq: u64,
	inclusive: bool,
}

/// Compaction filter used by hash_db, list_db, set_db and zset_db.
///
/// Metadata is the exact prefix of every collection sub-key, so it sorts before
/// the sub-keys when both are present in one compaction input. The filter
/// caches only the newest metadata version it observes in that ordered stream.
/// It deliberately performs no database reads: querying the database being
/// compacted from `filter()` can stall compaction and foreground scans. If
/// metadata is not part of the current input, the filter fails open.
pub struct CollectionCompactionFilter {
	pub(crate) data_type: DataType,
	current_user_key: Option<Bytes>,
	strongest_cutoff: Option<Cutoff>,
	compaction_clock_tick: i64,
	retention_min_seq: Option<u64>,
	reclaimed_count: u64,
}

impl CollectionCompactionFilter {
	/// Decode a top-level or sub-key to extract the user-key portion.
	/// Key format: key_len(u16 BE) + user_key + optional suffix.
	fn decode_user_key(key: &[u8]) -> Option<Bytes> {
		if key.len() < 2 {
			return None;
		}
		let mut buf = key;
		let key_len = buf.get_u16() as usize;
		if buf.len() < key_len {
			return None;
		}
		Some(Bytes::copy_from_slice(&buf[..key_len]))
	}

	fn select_user_key(&mut self, user_key: &Bytes) {
		if self.current_user_key.as_ref() == Some(user_key) {
			return;
		}
		self.current_user_key = Some(user_key.clone());
		self.strongest_cutoff = None;
	}

	fn is_valid_sub_key(&self, encoded_key: &[u8], user_key: &Bytes) -> bool {
		let prefix_len = 2 + user_key.len();
		let Some(suffix) = encoded_key.get(prefix_len..) else {
			return false;
		};
		match self.data_type {
			DataType::Hash | DataType::Set => {
				if suffix.len() < 4 {
					return false;
				}
				let mut remaining = suffix;
				let value_len = remaining.get_u32() as usize;
				remaining.len() == value_len
			}
			DataType::List => suffix.len() == 8,
			DataType::ZSet => match suffix.first() {
				Some(b'M') if suffix.len() >= 5 => {
					let mut remaining = &suffix[1..];
					let member_len = remaining.get_u32() as usize;
					remaining.len() == member_len
				}
				Some(b'S') => suffix.len() >= 9,
				_ => false,
			},
			DataType::String => false,
		}
	}

	fn add_cutoff(&mut self, candidate: Cutoff) {
		// A cutoff newer than the oldest protected snapshot might hide rows that
		// snapshot still needs. RetentionIterator preserves older exact metadata
		// versions, allowing one of them to provide a safe cutoff instead.
		if self
			.retention_min_seq
			.is_some_and(|min_seq| candidate.seq > min_seq)
		{
			return;
		}
		let replace = self.strongest_cutoff.is_none_or(|current| {
			candidate.seq > current.seq
				|| (candidate.seq == current.seq && candidate.inclusive && !current.inclusive)
		});
		if replace {
			self.strongest_cutoff = Some(candidate);
		}
	}

	fn observe_meta(&mut self, entry: &RowEntry, user_key: &Bytes) {
		let candidate = match &entry.value {
			ValueDeletable::Tombstone => Some(Cutoff {
				seq: entry.seq,
				inclusive: true,
			}),
			ValueDeletable::Value(encoded) => match AnyValue::decode(encoded) {
				Ok(value) if value.data_type() == self.data_type => {
					if entry
						.expire_ts
						.is_some_and(|expire_ts| expire_ts <= self.compaction_clock_tick)
					{
						Some(Cutoff {
							seq: entry.seq,
							inclusive: true,
						})
					} else {
						let version = value
							.version()
							.map(|version| if version == 0 { entry.seq } else { version })
							.unwrap_or(entry.seq);
						Some(Cutoff {
							seq: version,
							inclusive: false,
						})
					}
				}
				// The previous hash-only candidate could atomically replace local
				// metadata with a String. Treat that row as an explicit generation
				// barrier while upgrading such databases.
				Ok(value) if value.data_type() == DataType::String => Some(Cutoff {
					seq: entry.seq,
					inclusive: true,
				}),
				Ok(value) => {
					warn!(
						"[{:?}Filter] Keep[Unexpected local metadata type {:?}] key: {:?}",
						self.data_type,
						value.data_type(),
						user_key
					);
					None
				}
				Err(error) => {
					warn!(
						"[{:?}Filter] Keep[Decode metadata failed: {:?}] key: {:?}",
						self.data_type, error, user_key
					);
					None
				}
			},
			ValueDeletable::Merge(_) => None,
		};
		if let Some(candidate) = candidate {
			self.add_cutoff(candidate);
		}
	}
}

#[async_trait]
impl CompactionFilter for CollectionCompactionFilter {
	async fn filter(
		&mut self,
		entry: &RowEntry,
	) -> Result<CompactionFilterDecision, CompactionFilterError> {
		let Some(user_key) = Self::decode_user_key(&entry.key) else {
			debug!(
				"[{:?}Filter] Keep[Invalid key format] key: {:?}",
				self.data_type, entry.key
			);
			return Ok(CompactionFilterDecision::Keep);
		};
		self.select_user_key(&user_key);

		let meta_encoded_key = MetaKey::new(user_key.clone()).encode();
		if entry.key == meta_encoded_key {
			self.observe_meta(entry, &user_key);
			return Ok(CompactionFilterDecision::Keep);
		}
		if !self.is_valid_sub_key(&entry.key, &user_key) {
			debug!(
				"[{:?}Filter] Keep[Invalid collection sub-key] key: {:?}",
				self.data_type, entry.key
			);
			return Ok(CompactionFilterDecision::Keep);
		}

		if matches!(&entry.value, ValueDeletable::Tombstone) {
			return Ok(CompactionFilterDecision::Keep);
		}

		let should_delete = self.strongest_cutoff.is_some_and(|cutoff| {
			entry.seq < cutoff.seq || (cutoff.inclusive && entry.seq == cutoff.seq)
		});
		if should_delete {
			self.reclaimed_count = self.reclaimed_count.saturating_add(1);
			return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
		}

		Ok(CompactionFilterDecision::Keep)
	}

	async fn on_compaction_end(&mut self) -> Result<(), CompactionFilterError> {
		debug!(
			"[{:?}Filter] reclaimed {} stale collection rows",
			self.data_type, self.reclaimed_count
		);
		Ok(())
	}
}

pub struct CollectionCompactionFilterSupplier {
	pub data_type: DataType,
}

impl CollectionCompactionFilterSupplier {
	pub fn new(data_type: DataType) -> Self {
		Self { data_type }
	}
}

#[async_trait]
impl CompactionFilterSupplier for CollectionCompactionFilterSupplier {
	async fn create_compaction_filter(
		&self,
		context: &CompactionJobContext,
	) -> Result<Box<dyn CompactionFilter>, CompactionFilterError> {
		Ok(Box::new(CollectionCompactionFilter {
			data_type: self.data_type,
			current_user_key: None,
			strongest_cutoff: None,
			compaction_clock_tick: context.compaction_clock_tick,
			retention_min_seq: context.retention_min_seq,
			reclaimed_count: 0,
		}))
	}
}

#[cfg(test)]
mod tests {
	use bytes::BufMut;
	use bytes::BytesMut;

	use super::*;
	use crate::string::meta::HashMetaValue;
	use crate::string::meta::ListMetaValue;
	use crate::string::meta::SetMetaValue;
	use crate::string::meta::ZSetMetaValue;
	use crate::string::value::StringValue;

	fn filter(data_type: DataType) -> CollectionCompactionFilter {
		CollectionCompactionFilter {
			data_type,
			current_user_key: None,
			strongest_cutoff: None,
			compaction_clock_tick: 1_000,
			retention_min_seq: None,
			reclaimed_count: 0,
		}
	}

	fn row(key: Bytes, value: ValueDeletable, seq: u64, expire_ts: Option<i64>) -> RowEntry {
		RowEntry {
			key,
			value,
			seq,
			create_ts: None,
			expire_ts,
		}
	}

	fn sub_key(data_type: DataType, user_key: &Bytes, suffix: &[u8]) -> Bytes {
		let mut encoded = BytesMut::new();
		encoded.put_u16(user_key.len() as u16);
		encoded.extend_from_slice(user_key);
		match data_type {
			DataType::Hash | DataType::Set => {
				encoded.put_u32(suffix.len() as u32);
				encoded.extend_from_slice(suffix);
			}
			DataType::List => encoded.put_u64(1),
			DataType::ZSet => {
				encoded.put_u8(b'M');
				encoded.put_u32(suffix.len() as u32);
				encoded.extend_from_slice(suffix);
			}
			DataType::String => unreachable!(),
		}
		encoded.freeze()
	}

	#[tokio::test]
	async fn metadata_row_is_kept_and_controls_generation_for_every_collection() {
		let cases = [
			(DataType::Hash, HashMetaValue::new(10, 1).encode()),
			(DataType::List, ListMetaValue::new(10).encode()),
			(DataType::Set, SetMetaValue::new(10, 1).encode()),
			(DataType::ZSet, ZSetMetaValue::new(10, 1).encode()),
		];

		for (data_type, encoded_meta) in cases {
			let key = Bytes::from_static(b"collection");
			let mut filter = filter(data_type);
			let meta = row(
				MetaKey::new(key.clone()).encode(),
				ValueDeletable::Value(encoded_meta),
				20,
				None,
			);
			assert_eq!(
				filter.filter(&meta).await.unwrap(),
				CompactionFilterDecision::Keep
			);

			let stale = row(
				sub_key(data_type, &key, b"field"),
				ValueDeletable::Value(Bytes::new()),
				9,
				None,
			);
			assert_eq!(
				filter.filter(&stale).await.unwrap(),
				CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
			);

			let current = row(
				sub_key(data_type, &key, b"current"),
				ValueDeletable::Value(Bytes::new()),
				10,
				None,
			);
			assert_eq!(
				filter.filter(&current).await.unwrap(),
				CompactionFilterDecision::Keep
			);
		}
	}

	#[tokio::test]
	async fn pending_generation_uses_metadata_commit_sequence() {
		let key = Bytes::from_static(b"pending");
		let mut filter = filter(DataType::Hash);
		let meta = row(
			MetaKey::new(key.clone()).encode(),
			ValueDeletable::Value(HashMetaValue::new(0, 1).encode()),
			42,
			None,
		);
		filter.filter(&meta).await.unwrap();

		let stale = row(
			sub_key(DataType::Hash, &key, b"old"),
			ValueDeletable::Value(Bytes::new()),
			41,
			None,
		);
		assert_eq!(
			filter.filter(&stale).await.unwrap(),
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
		let current = row(
			sub_key(DataType::Hash, &key, b"new"),
			ValueDeletable::Value(Bytes::new()),
			42,
			None,
		);
		assert_eq!(
			filter.filter(&current).await.unwrap(),
			CompactionFilterDecision::Keep
		);
	}

	#[tokio::test]
	async fn missing_metadata_in_compaction_input_fails_open() {
		let key = Bytes::from_static(b"missing-meta");
		let entry = row(
			sub_key(DataType::Set, &key, b"field"),
			ValueDeletable::Value(Bytes::new()),
			1,
			None,
		);
		assert_eq!(
			filter(DataType::Set).filter(&entry).await.unwrap(),
			CompactionFilterDecision::Keep
		);
	}

	#[tokio::test]
	async fn older_metadata_versions_do_not_override_the_newest() {
		let key = Bytes::from_static(b"versions");
		let meta_key = MetaKey::new(key.clone()).encode();
		let mut filter = filter(DataType::Hash);
		let newest = row(
			meta_key.clone(),
			ValueDeletable::Value(HashMetaValue::new(100, 1).encode()),
			100,
			None,
		);
		let older = row(
			meta_key,
			ValueDeletable::Value(HashMetaValue::new(1, 1).encode()),
			1,
			None,
		);
		filter.filter(&newest).await.unwrap();
		filter.filter(&older).await.unwrap();

		let stale = row(
			sub_key(DataType::Hash, &key, b"old"),
			ValueDeletable::Value(Bytes::new()),
			50,
			None,
		);
		assert_eq!(
			filter.filter(&stale).await.unwrap(),
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
	}

	#[tokio::test]
	async fn deletion_barrier_only_reclaims_older_rows() {
		let key = Bytes::from_static(b"deleted");
		let mut filter = filter(DataType::Set);
		let deleted_meta = row(
			MetaKey::new(key.clone()).encode(),
			ValueDeletable::Tombstone,
			20,
			None,
		);
		filter.filter(&deleted_meta).await.unwrap();

		let older = row(
			sub_key(DataType::Set, &key, b"older"),
			ValueDeletable::Value(Bytes::new()),
			19,
			None,
		);
		assert_eq!(
			filter.filter(&older).await.unwrap(),
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
		let same_batch = row(
			sub_key(DataType::Set, &key, b"same-batch"),
			ValueDeletable::Value(Bytes::new()),
			20,
			None,
		);
		assert_eq!(
			filter.filter(&same_batch).await.unwrap(),
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
		let newer = row(
			sub_key(DataType::Set, &key, b"newer"),
			ValueDeletable::Value(Bytes::new()),
			21,
			None,
		);
		assert_eq!(
			filter.filter(&newer).await.unwrap(),
			CompactionFilterDecision::Keep
		);
	}

	#[tokio::test]
	async fn retention_boundary_uses_an_older_snapshot_safe_cutoff() {
		let key = Bytes::from_static(b"snapshot");
		let meta_key = MetaKey::new(key.clone()).encode();
		let mut filter = filter(DataType::Hash);
		filter.retention_min_seq = Some(50);

		let newer_than_snapshot = row(
			meta_key.clone(),
			ValueDeletable::Value(HashMetaValue::new(100, 1).encode()),
			100,
			None,
		);
		let visible_to_snapshot = row(
			meta_key,
			ValueDeletable::Value(HashMetaValue::new(20, 1).encode()),
			40,
			None,
		);
		filter.filter(&newer_than_snapshot).await.unwrap();
		filter.filter(&visible_to_snapshot).await.unwrap();

		let stale = row(
			sub_key(DataType::Hash, &key, b"stale"),
			ValueDeletable::Value(Bytes::new()),
			19,
			None,
		);
		assert_eq!(
			filter.filter(&stale).await.unwrap(),
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
		let snapshot_visible = row(
			sub_key(DataType::Hash, &key, b"visible"),
			ValueDeletable::Value(Bytes::new()),
			20,
			None,
		);
		assert_eq!(
			filter.filter(&snapshot_visible).await.unwrap(),
			CompactionFilterDecision::Keep
		);
	}

	#[tokio::test]
	async fn expiration_and_string_replacement_create_sequence_barriers() {
		let key = Bytes::from_static(b"barrier");
		let subkey = row(
			sub_key(DataType::Set, &key, b"member"),
			ValueDeletable::Value(Bytes::new()),
			1,
			None,
		);

		let mut expired_filter = filter(DataType::Set);
		let expired_meta = row(
			MetaKey::new(key.clone()).encode(),
			ValueDeletable::Value(SetMetaValue::new(1, 1).encode()),
			2,
			Some(999),
		);
		expired_filter.filter(&expired_meta).await.unwrap();
		assert_eq!(
			expired_filter.filter(&subkey).await.unwrap(),
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);

		let mut string_filter = filter(DataType::Hash);
		let string_meta = row(
			MetaKey::new(key).encode(),
			ValueDeletable::Value(StringValue::new("replacement").encode()),
			2,
			None,
		);
		string_filter.filter(&string_meta).await.unwrap();
		assert_eq!(
			string_filter.filter(&subkey).await.unwrap(),
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
	}
}
