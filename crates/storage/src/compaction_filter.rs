use std::sync::Arc;

use async_trait::async_trait;
use bytes::Buf;
use bytes::Bytes;
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
use crate::string::meta::AnyValue;
use crate::string::meta::MetaKey;

pub struct NimbisCompactionFilter {
	string_db: Option<Arc<Db>>,
	data_type: DataType,
}

impl NimbisCompactionFilter {
	fn decode_sub_key(key: &[u8]) -> Option<(Bytes, u64)> {
		if key.len() < 2 {
			return None;
		}
		let mut buf = key;
		let key_len = buf.get_u16() as usize;
		if buf.len() < key_len + 8 {
			return None;
		}
		let user_key = Bytes::copy_from_slice(&buf[..key_len]);
		buf.advance(key_len);
		let version = buf.get_u64();
		Some((user_key, version))
	}
}

#[async_trait]
impl CompactionFilter for NimbisCompactionFilter {
	async fn filter(
		&mut self,
		entry: &RowEntry,
	) -> Result<CompactionFilterDecision, CompactionFilterError> {
		// We only care about entries that are not already tombstones
		let bytes = match &entry.value {
			ValueDeletable::Value(bytes) | ValueDeletable::Merge(bytes) => bytes,
			ValueDeletable::Tombstone => return Ok(CompactionFilterDecision::Keep),
		};

		match self.data_type {
			DataType::String => {
				// String DB: Check TTL in value
				let any_val = match AnyValue::decode(bytes) {
					Ok(val) => val,
					Err(_) => return Ok(CompactionFilterDecision::Keep),
				};

				if any_val.is_expired() {
					return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
				}
				Ok(CompactionFilterDecision::Keep)
			}
			_ => {
				// Collection DB: Check metadata in string_db
				let Some(string_db) = &self.string_db else {
					// Should not happen if configured correctly
					return Ok(CompactionFilterDecision::Keep);
				};

				// Decode sub-key to getKey and Version
				let Some((user_key, version)) = Self::decode_sub_key(&entry.key) else {
					// Invalid key format? Keep safe.
					return Ok(CompactionFilterDecision::Keep);
				};

				// Lookup metadata
				let meta_key = MetaKey::new(user_key);
				let meta_encoded = match string_db.get(meta_key.encode()).await {
					Ok(Some(v)) => v,
					Ok(None) => {
						// Metadata missing -> Orphaned sub-key -> Delete
						return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
					}
					Err(_) => return Ok(CompactionFilterDecision::Keep), // Error -> Keep safe
				};

				let any_val = match AnyValue::decode(&meta_encoded) {
					Ok(v) => v,
					Err(_) => return Ok(CompactionFilterDecision::Keep),
				};

				// Check expiration
				if any_val.is_expired() {
					return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
				}

				// Check Type and Version
				match any_val {
					AnyValue::Hash(m) if self.data_type == DataType::Hash => {
						if m.version != version {
							return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
						}
					}
					AnyValue::List(m) if self.data_type == DataType::List => {
						if m.version != version {
							return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
						}
					}
					AnyValue::Set(m) if self.data_type == DataType::Set => {
						if m.version != version {
							return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
						}
					}
					AnyValue::ZSet(m) if self.data_type == DataType::ZSet => {
						if m.version != version {
							return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
						}
					}
					_ => {
						// Type mismatch -> Orphaned sub-key (collision) -> Delete
						return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
					}
				}

				Ok(CompactionFilterDecision::Keep)
			}
		}
	}

	async fn on_compaction_end(&mut self) -> Result<(), CompactionFilterError> {
		Ok(())
	}
}

pub struct NimbisCompactionFilterSupplier {
	pub string_db: Option<Arc<Db>>,
	pub data_type: DataType,
}

#[async_trait]
impl CompactionFilterSupplier for NimbisCompactionFilterSupplier {
	async fn create_compaction_filter(
		&self,
		_context: &CompactionJobContext,
	) -> Result<Box<dyn CompactionFilter>, CompactionFilterError> {
		Ok(Box::new(NimbisCompactionFilter {
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

	#[tokio::test]
	async fn test_filter_expired_key() {
		let mut filter = NimbisCompactionFilter {
			string_db: None,
			data_type: DataType::String,
		};
		let value = StringValue::new_with_ttl(Bytes::from("val"), 100);
		let entry = RowEntry {
			key: Bytes::from("key"),
			value: ValueDeletable::Value(value.encode()),
			seq: 1,
			create_ts: None,
			expire_ts: None,
		};

		// Mock current time > 100
		let decision = filter.filter(&entry).await.unwrap();
		// Since we use Utc::now() in AnyValue::is_expired(), we need to make sure 100
		// is past. 100ms since epoch is definitely in the past.
		assert_eq!(
			decision,
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
	}

	#[tokio::test]
	async fn test_filter_not_expired_key() {
		let mut filter = NimbisCompactionFilter {
			string_db: None,
			data_type: DataType::String,
		};
		let future_time = (chrono::Utc::now().timestamp_millis() + 100000) as u64;
		let value = StringValue::new_with_ttl(Bytes::from("val"), future_time);
		let entry = RowEntry {
			key: Bytes::from("key"),
			value: ValueDeletable::Value(value.encode()),
			seq: 1,
			create_ts: None,
			expire_ts: None,
		};

		let decision = filter.filter(&entry).await.unwrap();
		assert_eq!(decision, CompactionFilterDecision::Keep);
	}

	#[tokio::test]
	async fn test_filter_version_mismatch() {
		use std::sync::Arc;

		use bytes::BufMut;
		use bytes::BytesMut;
		use slatedb::Db;
		use slatedb::object_store::local::LocalFileSystem;
		use slatedb::object_store::path::Path;

		use crate::string::meta::HashMetaValue;

		// 1. Setup string_db using local temp dir
		let temp_dir = std::env::temp_dir().join(format!("nimbis-test-{}", ulid::Ulid::new()));
		tokio::fs::create_dir_all(&temp_dir).await.unwrap();
		let object_store = Arc::new(LocalFileSystem::new_with_prefix(&temp_dir).unwrap());

		let string_db = Db::builder(Path::from("/string"), object_store)
			.build()
			.await
			.unwrap();
		let string_db = Arc::new(string_db);

		// 2. Put Metadata (version 10)
		let user_key = Bytes::from("myhash");
		let meta_key = MetaKey::new(user_key.clone());
		let meta_val = HashMetaValue::new(10, 5);
		string_db
			.put(meta_key.encode(), meta_val.encode())
			.await
			.unwrap();

		// 3. Setup Filter
		let mut filter = NimbisCompactionFilter {
			string_db: Some(string_db.clone()),
			data_type: DataType::Hash,
		};

		// 4. Test Valid Version (10)
		// SubKey: len(2) + key + version(8) + field_len(4) + field
		let mut valid_key = BytesMut::new();
		valid_key.put_u16(user_key.len() as u16);
		valid_key.extend_from_slice(&user_key);
		valid_key.put_u64(10); // Matches
		valid_key.put_u32(5); // field len
		valid_key.put_slice(b"field");
		let valid_entry = RowEntry {
			key: valid_key.freeze(),
			value: ValueDeletable::Value(Bytes::from("val")),
			seq: 1,
			create_ts: None,
			expire_ts: None,
		};
		assert_eq!(
			filter.filter(&valid_entry).await.unwrap(),
			CompactionFilterDecision::Keep
		);

		// 5. Test Invalid Version (9)
		let mut invalid_key = BytesMut::new();
		invalid_key.put_u16(user_key.len() as u16);
		invalid_key.extend_from_slice(&user_key);
		invalid_key.put_u64(9); // Mismatch!
		invalid_key.put_u32(5);
		invalid_key.put_slice(b"field");
		let invalid_entry = RowEntry {
			key: invalid_key.freeze(),
			value: ValueDeletable::Value(Bytes::from("val")),
			seq: 2,
			create_ts: None,
			expire_ts: None,
		};
		assert_eq!(
			filter.filter(&invalid_entry).await.unwrap(),
			CompactionFilterDecision::Modify(ValueDeletable::Tombstone)
		);
	}
}
