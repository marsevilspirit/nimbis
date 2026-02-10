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
use crate::string::meta::AnyValue;
use crate::string::meta::MetaKey;

pub struct NimbisCompactionFilter {
	pub(crate) string_db: Option<Arc<Db>>,
	pub(crate) data_type: DataType,
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
					Err(e) => {
						debug!(
							"[StringFilter] failed to decode value for key {:?}: {:?}",
							entry.key, e
						);
						return Ok(CompactionFilterDecision::Keep);
					}
				};

				if any_val.is_expired() {
					debug!("[StringFilter] Drop[Stale] key: {:?}", entry.key);
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
					debug!(
						"[{:?}Filter] Invalid key format: {:?}",
						self.data_type, entry.key
					);
					return Ok(CompactionFilterDecision::Keep);
				};

				// Lookup metadata
				let meta_key = MetaKey::new(user_key.clone());
				let meta_encoded = match string_db.get(meta_key.encode()).await {
					Ok(Some(v)) => v,
					Ok(None) => {
						// Metadata missing -> Orphaned sub-key -> Delete
						debug!(
							"[{:?}DataFilter] Drop[Meta key not exist] key: {:?}, version: {}",
							self.data_type, user_key, version
						);
						return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
					}
					Err(e) => {
						debug!(
							"[{:?}DataFilter] Reserve[Get meta_key failed: {:?}] key: {:?}",
							self.data_type, e, user_key
						);
						return Ok(CompactionFilterDecision::Keep);
					}
				};

				let any_val = match AnyValue::decode(&meta_encoded) {
					Ok(v) => v,
					Err(e) => {
						debug!(
							"[{:?}DataFilter] Reserve[Decode meta failed: {:?}] key: {:?}",
							self.data_type, e, user_key
						);
						return Ok(CompactionFilterDecision::Keep);
					}
				};

				// Check expiration
				if any_val.is_expired() {
					debug!(
						"[{:?}DataFilter] Drop[Timeout] key: {:?}, version: {}",
						self.data_type, user_key, version
					);
					return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
				}

				// Check Type
				if any_val.data_type() != self.data_type {
					// Type mismatch -> Orphaned sub-key (collision) -> Delete
					debug!(
						"[{:?}DataFilter] Drop[Type mismatch: expected {:?}, found {:?}] key: {:?}, version: {}",
						self.data_type,
						self.data_type,
						any_val.data_type(),
						user_key,
						version
					);
					return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
				}

				// Check Version
				if let Some(meta_version) = any_val.version()
					&& meta_version != version
				{
					debug!(
						"[{:?}DataFilter] Drop[version mismatch: cur_meta_version {}, data_key_version {}] key: {:?}",
						self.data_type, meta_version, version, user_key
					);
					return Ok(CompactionFilterDecision::Modify(ValueDeletable::Tombstone));
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

	#[tokio::test]
	async fn test_compaction_reclaims_orphaned_data() {
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

		// Build 3 sub-keys with version=42 (matching)
		let mut filter = NimbisCompactionFilter {
			string_db: Some(string_db.clone()),
			data_type: DataType::Set,
		};

		let build_sub_key = |version: u64, member: &[u8]| -> Bytes {
			let mut key = BytesMut::new();
			key.put_u16(user_key.len() as u16);
			key.extend_from_slice(&user_key);
			key.put_u64(version);
			key.put_u32(member.len() as u32);
			key.extend_from_slice(member);
			key.freeze()
		};

		let members: &[&[u8]] = &[b"alice", b"bob", b"carol"];
		for member in members {
			let entry = RowEntry {
				key: build_sub_key(42, *member),
				value: ValueDeletable::Value(Bytes::new()),
				seq: 1,
				create_ts: None,
				expire_ts: None,
			};
			// All should be kept (version matches)
			assert_eq!(
				filter.filter(&entry).await.unwrap(),
				CompactionFilterDecision::Keep,
				"member {:?} should be kept when version matches",
				std::str::from_utf8(*member).unwrap()
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
				key: build_sub_key(42, *member),
				value: ValueDeletable::Value(Bytes::new()),
				seq: 1,
				create_ts: None,
				expire_ts: None,
			};
			assert_eq!(
				filter.filter(&entry).await.unwrap(),
				CompactionFilterDecision::Modify(ValueDeletable::Tombstone),
				"member {:?} should be reclaimed after metadata deletion",
				std::str::from_utf8(*member).unwrap()
			);
		}

		// Simulate re-creation with new version: put meta with version=100
		let new_meta_val = SetMetaValue::new(100, 1);
		string_db
			.put(meta_key.encode(), new_meta_val.encode())
			.await
			.unwrap();

		// Old version=42 data should still be reclaimed
		for member in members {
			let entry = RowEntry {
				key: build_sub_key(42, *member),
				value: ValueDeletable::Value(Bytes::new()),
				seq: 1,
				create_ts: None,
				expire_ts: None,
			};
			assert_eq!(
				filter.filter(&entry).await.unwrap(),
				CompactionFilterDecision::Modify(ValueDeletable::Tombstone),
				"old version member {:?} should be reclaimed after re-creation",
				std::str::from_utf8(*member).unwrap()
			);
		}

		// New version=100 data should be kept
		let new_entry = RowEntry {
			key: build_sub_key(100, b"dave"),
			value: ValueDeletable::Value(Bytes::new()),
			seq: 2,
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
