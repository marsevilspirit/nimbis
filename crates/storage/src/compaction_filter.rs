use std::sync::Arc;

use async_trait::async_trait;
use bytes::Buf;
use bytes::Bytes;
use log::debug;
use slatedb::CompactionFilter;
use slatedb::CompactionFilterDecision;
use slatedb::CompactionFilterError;
use slatedb::CompactionFilterSupplier;
use slatedb::CompactionJobContext;
use slatedb::Db;
use slatedb::RowEntry;
use slatedb::ValueDeletable;

use crate::data_type::DataType;
use crate::expirable::Expirable;
use crate::storage::Storage;
use crate::string::meta::AnyValue;
use crate::string::meta::MetaKey;

// ---------------------------------------------------------------------------
// StringCompactionFilter — used exclusively by string_db
// ---------------------------------------------------------------------------

pub struct StringCompactionFilter;

#[async_trait]
impl CompactionFilter for StringCompactionFilter {
	async fn filter(
		&mut self,
		entry: &RowEntry,
	) -> Result<CompactionFilterDecision, CompactionFilterError> {
		// Skip tombstones
		let _bytes = match &entry.value {
			ValueDeletable::Value(bytes) | ValueDeletable::Merge(bytes) => bytes,
			ValueDeletable::Tombstone => return Ok(CompactionFilterDecision::Keep),
		};

		// Check expiration from SlateDB metadata for all types stored in string_db
		// (String values, Hash/Set/List/ZSet metadata)
		if Storage::is_expired(entry.expire_ts) {
			debug!("[StringFilter] Drop[Stale] key: {:?}", entry.key);
			return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
		}

		Ok(CompactionFilterDecision::Keep)
	}

	async fn on_compaction_end(&mut self) -> Result<(), CompactionFilterError> {
		Ok(())
	}
}

pub struct StringCompactionFilterSupplier;

#[async_trait]
impl CompactionFilterSupplier for StringCompactionFilterSupplier {
	async fn create_compaction_filter(
		&self,
		_context: &CompactionJobContext,
	) -> Result<Box<dyn CompactionFilter>, CompactionFilterError> {
		Ok(Box::new(StringCompactionFilter))
	}
}

// ---------------------------------------------------------------------------
// CollectionCompactionFilter — used by hash_db, list_db, set_db, zset_db
// ---------------------------------------------------------------------------

pub struct CollectionCompactionFilter {
	pub(crate) string_db: Arc<Db>,
	pub(crate) data_type: DataType,
}

impl CollectionCompactionFilter {
	/// Decode a sub-key to extract the user_key portion.
	/// Sub-key format: key_len(u16 BE) + user_key + ...
	fn decode_sub_key(key: &[u8]) -> Option<Bytes> {
		if key.len() < 2 {
			return None;
		}
		let mut buf = key;
		let key_len = buf.get_u16() as usize;
		if buf.len() < key_len {
			return None;
		}
		let user_key = Bytes::copy_from_slice(&buf[..key_len]);
		Some(user_key)
	}
}

#[async_trait]
impl CompactionFilter for CollectionCompactionFilter {
	async fn filter(
		&mut self,
		entry: &RowEntry,
	) -> Result<CompactionFilterDecision, CompactionFilterError> {
		// Skip tombstones
		let _bytes = match &entry.value {
			ValueDeletable::Value(bytes) | ValueDeletable::Merge(bytes) => bytes,
			ValueDeletable::Tombstone => return Ok(CompactionFilterDecision::Keep),
		};

		// Decode sub-key to get user_key
		let Some(user_key) = Self::decode_sub_key(&entry.key) else {
			debug!(
				"[{:?}Filter] Invalid key format: {:?}",
				self.data_type, entry.key
			);
			return Ok(CompactionFilterDecision::Keep);
		};

		// Lookup metadata in string_db
		let meta_key = MetaKey::new(user_key.clone());
		let kv = match self.string_db.get_key_value(meta_key.encode()).await {
			Ok(Some(v)) => v,
			Ok(None) => {
				// Metadata missing -> Orphaned sub-key -> Delete
				debug!(
					"[{:?}Filter] Drop[Meta missing] key: {:?}",
					self.data_type, user_key
				);
				return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
			}
			Err(e) => {
				debug!(
					"[{:?}Filter] Keep[Get meta failed: {:?}] key: {:?}",
					self.data_type, e, user_key
				);
				return Ok(CompactionFilterDecision::Keep);
			}
		};

		// Consider expired metadata as non-existent to allow sub-key cleanup
		if Storage::is_expired(kv.expire_ts) {
			debug!(
				"[{:?}Filter] Drop[Meta expired] key: {:?}",
				self.data_type, user_key
			);
			return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
		}

		let meta_encoded = kv.value;

		let any_val = match AnyValue::decode(&meta_encoded) {
			Ok(v) => v,
			Err(e) => {
				debug!(
					"[{:?}Filter] Keep[Decode meta failed: {:?}] key: {:?}",
					self.data_type, e, user_key
				);
				return Ok(CompactionFilterDecision::Keep);
			}
		};

		// Check expiration from decoded metadata
		if any_val.is_expired() {
			debug!(
				"[{:?}Filter] Drop[Timeout] key: {:?}",
				self.data_type, user_key
			);
			return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
		}

		// Check type — type mismatch means orphaned sub-key from a type collision
		if any_val.data_type() != self.data_type {
			debug!(
				"[{:?}Filter] Drop[Type mismatch: expected {:?}, found {:?}] key: {:?}",
				self.data_type,
				self.data_type,
				any_val.data_type(),
				user_key
			);
			return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
		}

		// Check version — old generation sub-keys should be tombstoned
		if let Some(meta_version) = any_val.version()
			&& entry.seq < meta_version
		{
			debug!(
				"[{:?}Filter] Drop[old seq: meta_version {}, data_seq {}] key: {:?}",
				self.data_type, meta_version, entry.seq, user_key
			);
			return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
		}

		Ok(CompactionFilterDecision::Keep)
	}

	async fn on_compaction_end(&mut self) -> Result<(), CompactionFilterError> {
		Ok(())
	}
}

pub struct CollectionCompactionFilterSupplier {
	pub string_db: Arc<Db>,
	pub data_type: DataType,
}

#[async_trait]
impl CompactionFilterSupplier for CollectionCompactionFilterSupplier {
	async fn create_compaction_filter(
		&self,
		_context: &CompactionJobContext,
	) -> Result<Box<dyn CompactionFilter>, CompactionFilterError> {
		Ok(Box::new(CollectionCompactionFilter {
			string_db: self.string_db.clone(),
			data_type: self.data_type,
		}))
	}
}

#[cfg(test)]
mod tests {
	use bytes::Bytes;
	use slatedb::ValueDeletable;

	use super::*;
	use crate::string::value::StringValue;

	// -----------------------------------------------------------------------
	// StringCompactionFilter tests
	// -----------------------------------------------------------------------

	#[tokio::test]
	async fn test_string_filter_expired_key() {
		let mut filter = StringCompactionFilter;
		let value = StringValue::new(Bytes::from("val"));
		let entry = RowEntry {
			key: Bytes::from("key"),
			value: ValueDeletable::Value(value.encode()),
			seq: 1,
			create_ts: None,
			expire_ts: Some(100),
		};

		let decision = filter.filter(&entry).await.unwrap();
		assert_eq!(
			decision,
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
	}

	#[tokio::test]
	async fn test_string_filter_not_expired_key() {
		let mut filter = StringCompactionFilter;
		let future_time = chrono::Utc::now().timestamp_millis() + 100000;
		let value = StringValue::new(Bytes::from("val"));
		let entry = RowEntry {
			key: Bytes::from("key"),
			value: ValueDeletable::Value(value.encode()),
			seq: 1,
			create_ts: None,
			expire_ts: Some(future_time),
		};

		let decision = filter.filter(&entry).await.unwrap();
		assert_eq!(decision, CompactionFilterDecision::Keep);
	}

	#[tokio::test]
	async fn test_string_filter_keeps_collection_meta() {
		use crate::string::meta::HashMetaValue;

		let mut filter = StringCompactionFilter;
		// HashMetaValue is a non-String type stored in string_db — should be kept
		let meta_value = HashMetaValue::new(1, 1).encode();
		let entry = RowEntry {
			key: Bytes::from("hash-meta-key"),
			value: ValueDeletable::Value(meta_value),
			seq: 1,
			create_ts: None,
			expire_ts: None,
		};

		let decision = filter.filter(&entry).await.unwrap();
		assert_eq!(decision, CompactionFilterDecision::Keep);
	}

	#[tokio::test]
	async fn test_string_filter_drops_expired_collection_meta() {
		use crate::string::meta::HashMetaValue;

		let mut filter = StringCompactionFilter;
		let meta_value = HashMetaValue::new(1, 1).encode();
		let past_time = chrono::Utc::now().timestamp_millis() - 60000;

		let entry = RowEntry {
			key: Bytes::from("expired-meta-key"),
			value: ValueDeletable::Value(meta_value),
			seq: 1,
			create_ts: None,
			expire_ts: Some(past_time),
		};

		let decision = filter.filter(&entry).await.unwrap();
		assert_eq!(
			decision,
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
	}

	// -----------------------------------------------------------------------
	// CollectionCompactionFilter tests
	// -----------------------------------------------------------------------

	#[tokio::test]
	async fn test_collection_filter_version_mismatch() {
		use std::sync::Arc;

		use bytes::BufMut;
		use bytes::BytesMut;
		use slatedb::Db;
		use slatedb::object_store::local::LocalFileSystem;
		use slatedb::object_store::path::Path;

		use crate::string::meta::HashMetaValue;

		// Setup string_db using local temp dir
		let temp_dir = std::env::temp_dir().join(format!("nimbis-test-{}", ulid::Ulid::new()));
		tokio::fs::create_dir_all(&temp_dir).await.unwrap();
		let object_store = Arc::new(LocalFileSystem::new_with_prefix(&temp_dir).unwrap());

		let string_db = Db::builder(Path::from("/string"), object_store)
			.build()
			.await
			.unwrap();
		let string_db = Arc::new(string_db);

		// Put Metadata (version 10)
		let user_key = Bytes::from("myhash");
		let meta_key = MetaKey::new(user_key.clone());
		let meta_val = HashMetaValue::new(10, 5);
		string_db
			.put(meta_key.encode(), meta_val.encode())
			.await
			.unwrap();

		// Setup Filter
		let mut filter = CollectionCompactionFilter {
			string_db: string_db.clone(),
			data_type: DataType::Hash,
		};

		// Test Valid seq (10)
		let mut valid_key = BytesMut::new();
		valid_key.put_u16(user_key.len() as u16);
		valid_key.extend_from_slice(&user_key);
		valid_key.put_u32(5); // field len
		valid_key.put_slice(b"field");
		let valid_entry = RowEntry {
			key: valid_key.freeze(),
			value: ValueDeletable::Value(Bytes::from("val")),
			seq: 10,
			create_ts: None,
			expire_ts: None,
		};
		assert_eq!(
			filter.filter(&valid_entry).await.unwrap(),
			CompactionFilterDecision::Keep
		);

		// Test Invalid seq (9)
		let mut invalid_key = BytesMut::new();
		invalid_key.put_u16(user_key.len() as u16);
		invalid_key.extend_from_slice(&user_key);
		invalid_key.put_u32(5);
		invalid_key.put_slice(b"field");
		let invalid_entry = RowEntry {
			key: invalid_key.freeze(),
			value: ValueDeletable::Value(Bytes::from("val")),
			seq: 9,
			create_ts: None,
			expire_ts: None,
		};
		assert_eq!(
			filter.filter(&invalid_entry).await.unwrap(),
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
	}

	#[tokio::test]
	async fn test_collection_filter_reclaims_orphaned_data() {
		use std::sync::Arc;

		use bytes::BufMut;
		use bytes::BytesMut;
		use slatedb::Db;
		use slatedb::config::WriteOptions;
		use slatedb::object_store::local::LocalFileSystem;
		use slatedb::object_store::path::Path;

		use crate::string::meta::SetMetaValue;

		// Setup string_db
		let temp_dir = std::env::temp_dir().join(format!("nimbis-test-{}", ulid::Ulid::new()));
		tokio::fs::create_dir_all(&temp_dir).await.unwrap();
		let object_store = Arc::new(LocalFileSystem::new_with_prefix(&temp_dir).unwrap());

		let string_db = Db::builder(Path::from("/string"), object_store)
			.build()
			.await
			.unwrap();
		let string_db = Arc::new(string_db);

		// Put Metadata for a Set (version=42, len=3)
		let user_key = Bytes::from("myset");
		let meta_key = MetaKey::new(user_key.clone());
		let meta_val = SetMetaValue::new(42, 3);
		string_db
			.put(meta_key.encode(), meta_val.encode())
			.await
			.unwrap();

		let mut filter = CollectionCompactionFilter {
			string_db: string_db.clone(),
			data_type: DataType::Set,
		};

		let build_sub_key = |member: &[u8]| -> Bytes {
			let mut key = BytesMut::new();
			key.put_u16(user_key.len() as u16);
			key.extend_from_slice(&user_key);
			key.put_u32(member.len() as u32);
			key.extend_from_slice(member);
			key.freeze()
		};

		let members: &[&[u8]] = &[b"alice", b"bob", b"carol"];
		for member in members {
			let entry = RowEntry {
				key: build_sub_key(member),
				value: ValueDeletable::Value(Bytes::new()),
				seq: 42,
				create_ts: None,
				expire_ts: None,
			};
			assert_eq!(
				filter.filter(&entry).await.unwrap(),
				CompactionFilterDecision::Keep,
				"member {:?} should be kept when version matches",
				std::str::from_utf8(member).unwrap()
			);
		}

		// Simulate DEL: remove the metadata
		let write_opts = WriteOptions {
			await_durable: false,
		};
		string_db
			.delete_with_options(meta_key.encode(), &write_opts)
			.await
			.unwrap();

		// Now all sub-keys should be marked for deletion (orphaned)
		for member in members {
			let entry = RowEntry {
				key: build_sub_key(member),
				value: ValueDeletable::Value(Bytes::new()),
				seq: 42,
				create_ts: None,
				expire_ts: None,
			};
			assert_eq!(
				filter.filter(&entry).await.unwrap(),
				CompactionFilterDecision::Modify(ValueDeletable::Tombstone),
				"member {:?} should be reclaimed after metadata deletion",
				std::str::from_utf8(member).unwrap()
			);
		}

		// Simulate re-creation with new version: put meta with version=100
		let new_meta_val = SetMetaValue::new(100, 1);
		string_db
			.put(meta_key.encode(), new_meta_val.encode())
			.await
			.unwrap();

		// Old seq=42 data should still be reclaimed
		for member in members {
			let entry = RowEntry {
				key: build_sub_key(member),
				value: ValueDeletable::Value(Bytes::new()),
				seq: 42,
				create_ts: None,
				expire_ts: None,
			};
			assert_eq!(
				filter.filter(&entry).await.unwrap(),
				CompactionFilterDecision::Modify(ValueDeletable::Tombstone),
				"old version member {:?} should be reclaimed after re-creation",
				std::str::from_utf8(member).unwrap()
			);
		}

		// New seq=100 data should be kept
		let new_entry = RowEntry {
			key: build_sub_key(b"dave"),
			value: ValueDeletable::Value(Bytes::new()),
			seq: 100,
			create_ts: None,
			expire_ts: None,
		};
		assert_eq!(
			filter.filter(&new_entry).await.unwrap(),
			CompactionFilterDecision::Keep,
			"new version member should be kept"
		);
	}
}
