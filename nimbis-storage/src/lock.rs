use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::OwnedRwLockReadGuard;
use tokio::sync::OwnedRwLockWriteGuard;
use tokio::sync::RwLock;

use crate::data_type::DataType;

const DEFAULT_KEY_LOCK_STRIPES: usize = 4096;
const KEY_LOCK_NAMESPACE_COUNT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StorageLockMode {
	Keys,
	GlobalWrite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageLock {
	mode: StorageLockMode,
	read_keys: Vec<KeyLockIdentity>,
	write_keys: Vec<KeyLockIdentity>,
}

impl StorageLock {
	pub(crate) fn global_write() -> Self {
		Self {
			mode: StorageLockMode::GlobalWrite,
			read_keys: Vec::new(),
			write_keys: Vec::new(),
		}
	}

	pub(crate) fn read_keys<I>(data_type: DataType, keys: I) -> Self
	where
		I: IntoIterator<Item = Bytes>,
	{
		Self {
			mode: StorageLockMode::Keys,
			read_keys: keys
				.into_iter()
				.map(|key| KeyLockIdentity { data_type, key })
				.collect(),
			write_keys: Vec::new(),
		}
	}

	pub(crate) fn write_keys<I>(data_type: DataType, keys: I) -> Self
	where
		I: IntoIterator<Item = Bytes>,
	{
		Self {
			mode: StorageLockMode::Keys,
			read_keys: Vec::new(),
			write_keys: keys
				.into_iter()
				.map(|key| KeyLockIdentity { data_type, key })
				.collect(),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyLockIdentity {
	data_type: DataType,
	key: Bytes,
}

#[derive(Debug)]
pub(crate) struct StorageLocks {
	db_lock: Arc<RwLock<()>>,
	key_locks: Arc<[Vec<Arc<RwLock<()>>>; KEY_LOCK_NAMESPACE_COUNT]>,
}

impl Default for StorageLocks {
	fn default() -> Self {
		let key_locks = std::array::from_fn(|_| {
			(0..DEFAULT_KEY_LOCK_STRIPES)
				.map(|_| Arc::new(RwLock::new(())))
				.collect()
		});

		Self {
			db_lock: Arc::new(RwLock::new(())),
			key_locks: Arc::new(key_locks),
		}
	}
}

impl StorageLocks {
	pub(crate) fn new() -> Self {
		Self::default()
	}

	pub(crate) async fn acquire(&self, lock: &StorageLock) -> StorageLockGuard {
		match lock.mode {
			StorageLockMode::GlobalWrite => StorageLockGuard {
				_db_read_guard: None,
				_db_write_guard: Some(self.db_lock.clone().write_owned().await),
				_key_guards: Vec::new(),
			},
			StorageLockMode::Keys => self.acquire_key_locks(lock).await,
		}
	}

	async fn acquire_key_locks(&self, lock: &StorageLock) -> StorageLockGuard {
		let db_read_guard = self.db_lock.clone().read_owned().await;
		let key_stripes = ordered_key_stripes(lock, DEFAULT_KEY_LOCK_STRIPES);
		let mut key_guards = Vec::with_capacity(key_stripes.len());

		for ((namespace, stripe), mode) in key_stripes {
			let lock = self.key_locks[namespace][stripe].clone();
			match mode {
				KeyMode::Read => key_guards.push(KeyLockGuard::Read {
					_guard: lock.read_owned().await,
				}),
				KeyMode::Write => key_guards.push(KeyLockGuard::Write {
					_guard: lock.write_owned().await,
				}),
			}
		}

		StorageLockGuard {
			_db_read_guard: Some(db_read_guard),
			_db_write_guard: None,
			_key_guards: key_guards,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyMode {
	Read,
	Write,
}

fn ordered_key_stripes(lock: &StorageLock, stripe_count: usize) -> Vec<((usize, usize), KeyMode)> {
	let mut stripes = BTreeMap::new();
	for key in &lock.read_keys {
		stripes
			.entry(lock_stripe(key, stripe_count))
			.or_insert(KeyMode::Read);
	}
	for key in &lock.write_keys {
		stripes.insert(lock_stripe(key, stripe_count), KeyMode::Write);
	}
	stripes.into_iter().collect()
}

fn lock_stripe(identity: &KeyLockIdentity, stripe_count: usize) -> (usize, usize) {
	(
		data_type_namespace(identity.data_type),
		stripe_index(&identity.key, stripe_count),
	)
}

fn data_type_namespace(data_type: DataType) -> usize {
	match data_type {
		DataType::String => 0,
		DataType::Hash => 1,
		DataType::List => 2,
		DataType::Set => 3,
		DataType::ZSet => 4,
	}
}

fn stripe_index(key: &Bytes, stripe_count: usize) -> usize {
	let mut hasher = DefaultHasher::new();
	key.hash(&mut hasher);
	hasher.finish() as usize % stripe_count
}

#[derive(Default)]
pub(crate) struct StorageLockGuard {
	_db_read_guard: Option<OwnedRwLockReadGuard<()>>,
	_db_write_guard: Option<OwnedRwLockWriteGuard<()>>,
	_key_guards: Vec<KeyLockGuard>,
}

enum KeyLockGuard {
	Read { _guard: OwnedRwLockReadGuard<()> },
	Write { _guard: OwnedRwLockWriteGuard<()> },
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use bytes::Bytes;
	use tokio::sync::Barrier;

	use super::*;

	#[tokio::test]
	async fn read_locks_can_overlap_for_the_same_key() {
		let locks = StorageLocks::new();
		let first = locks
			.acquire(&StorageLock::read_keys(
				DataType::String,
				[Bytes::from_static(b"key")],
			))
			.await;
		let second = tokio::time::timeout(
			Duration::from_millis(50),
			locks.acquire(&StorageLock::read_keys(
				DataType::String,
				[Bytes::from_static(b"key")],
			)),
		)
		.await;

		drop(first);
		assert!(second.is_ok(), "read/read locking should not block");
	}

	#[tokio::test]
	async fn write_lock_excludes_same_key_readers() {
		let locks = StorageLocks::new();
		let write_guard = locks
			.acquire(&StorageLock::write_keys(
				DataType::String,
				[Bytes::from_static(b"key")],
			))
			.await;

		let blocked = tokio::time::timeout(
			Duration::from_millis(50),
			locks.acquire(&StorageLock::read_keys(
				DataType::String,
				[Bytes::from_static(b"key")],
			)),
		)
		.await;

		drop(write_guard);
		assert!(blocked.is_err(), "write lock should block same-key readers");
	}

	#[tokio::test]
	async fn same_named_keys_in_different_types_do_not_block_each_other() {
		let locks = StorageLocks::new();
		let string_guard = locks
			.acquire(&StorageLock::write_keys(
				DataType::String,
				[Bytes::from_static(b"key")],
			))
			.await;

		let hash_guard = tokio::time::timeout(
			Duration::from_millis(50),
			locks.acquire(&StorageLock::write_keys(
				DataType::Hash,
				[Bytes::from_static(b"key")],
			)),
		)
		.await;

		drop(string_guard);
		assert!(
			hash_guard.is_ok(),
			"same-named keys in independent type namespaces should not block",
		);
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
	async fn multi_key_locks_use_stable_order_and_do_not_deadlock() {
		let locks = Arc::new(StorageLocks::new());
		let barrier = Arc::new(Barrier::new(2));

		let left = {
			let locks = locks.clone();
			let barrier = barrier.clone();
			tokio::spawn(async move {
				barrier.wait().await;
				let guard = locks
					.acquire(&StorageLock::write_keys(
						DataType::Hash,
						[Bytes::from_static(b"a"), Bytes::from_static(b"b")],
					))
					.await;
				drop(guard);
			})
		};

		let right = {
			let locks = locks.clone();
			let barrier = barrier.clone();
			tokio::spawn(async move {
				barrier.wait().await;
				let guard = locks
					.acquire(&StorageLock::write_keys(
						DataType::Hash,
						[Bytes::from_static(b"b"), Bytes::from_static(b"a")],
					))
					.await;
				drop(guard);
			})
		};

		let result = tokio::time::timeout(Duration::from_secs(1), async {
			left.await.expect("left lock task");
			right.await.expect("right lock task");
		})
		.await;

		assert!(result.is_ok(), "reverse multi-key locking should finish");
	}

	#[tokio::test]
	async fn global_write_lock_blocks_key_locks() {
		let locks = StorageLocks::new();
		let global_guard = locks.acquire(&StorageLock::global_write()).await;

		let blocked = tokio::time::timeout(
			Duration::from_millis(50),
			locks.acquire(&StorageLock::write_keys(
				DataType::String,
				[Bytes::from_static(b"key")],
			)),
		)
		.await;

		drop(global_guard);
		assert!(blocked.is_err(), "global write should block key locks");
	}

	#[tokio::test]
	async fn key_lock_table_is_bounded_for_many_unique_keys() {
		let locks = StorageLocks::new();
		let lock_slots = locks.key_locks[0].len();

		for i in 0..=lock_slots {
			let guard = locks
				.acquire(&StorageLock::write_keys(
					DataType::String,
					[Bytes::from(format!("key-{i}"))],
				))
				.await;
			drop(guard);
		}

		assert!(
			locks
				.key_locks
				.iter()
				.all(|namespace| namespace.len() == lock_slots)
		);
	}
}
