use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::OwnedRwLockReadGuard;
use tokio::sync::OwnedRwLockWriteGuard;
use tokio::sync::RwLock;

const DEFAULT_KEY_LOCK_STRIPES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageLockMode {
	None,
	Keys,
	GlobalWrite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLock {
	pub mode: StorageLockMode,
	pub read_keys: Vec<Bytes>,
	pub write_keys: Vec<Bytes>,
}

impl StorageLock {
	pub fn none() -> Self {
		Self {
			mode: StorageLockMode::None,
			read_keys: Vec::new(),
			write_keys: Vec::new(),
		}
	}

	pub fn global_write() -> Self {
		Self {
			mode: StorageLockMode::GlobalWrite,
			read_keys: Vec::new(),
			write_keys: Vec::new(),
		}
	}

	pub fn read_keys<I>(keys: I) -> Self
	where
		I: IntoIterator<Item = Bytes>,
	{
		Self {
			mode: StorageLockMode::Keys,
			read_keys: keys.into_iter().collect(),
			write_keys: Vec::new(),
		}
	}

	pub fn write_keys<I>(keys: I) -> Self
	where
		I: IntoIterator<Item = Bytes>,
	{
		Self {
			mode: StorageLockMode::Keys,
			read_keys: Vec::new(),
			write_keys: keys.into_iter().collect(),
		}
	}
}

#[derive(Debug)]
pub struct StorageLocks {
	db_lock: Arc<RwLock<()>>,
	key_locks: Arc<Vec<Arc<RwLock<()>>>>,
}

impl Default for StorageLocks {
	fn default() -> Self {
		let key_locks = (0..DEFAULT_KEY_LOCK_STRIPES)
			.map(|_| Arc::new(RwLock::new(())))
			.collect();

		Self {
			db_lock: Arc::new(RwLock::new(())),
			key_locks: Arc::new(key_locks),
		}
	}
}

impl StorageLocks {
	pub fn new() -> Self {
		Self::default()
	}

	pub async fn acquire(&self, lock: &StorageLock) -> StorageLockGuard {
		match lock.mode {
			StorageLockMode::None => StorageLockGuard::default(),
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
		let key_stripes = ordered_key_stripes(lock, self.key_locks.len());
		let mut key_guards = Vec::with_capacity(key_stripes.len());

		for (stripe, mode) in key_stripes {
			let lock = self.key_locks[stripe].clone();
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

fn ordered_key_stripes(lock: &StorageLock, stripe_count: usize) -> Vec<(usize, KeyMode)> {
	let mut stripes = BTreeMap::new();
	for key in &lock.read_keys {
		stripes
			.entry(stripe_index(key, stripe_count))
			.or_insert(KeyMode::Read);
	}
	for key in &lock.write_keys {
		stripes.insert(stripe_index(key, stripe_count), KeyMode::Write);
	}
	stripes.into_iter().collect()
}

fn stripe_index(key: &Bytes, stripe_count: usize) -> usize {
	let mut hasher = DefaultHasher::new();
	key.hash(&mut hasher);
	hasher.finish() as usize % stripe_count
}

#[derive(Default)]
pub struct StorageLockGuard {
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
	use bytes::Bytes;

	use super::*;

	#[tokio::test]
	async fn key_lock_table_is_bounded_for_many_unique_keys() {
		let locks = StorageLocks::new();
		let lock_slots = locks.key_locks.len();

		for i in 0..=lock_slots {
			let guard = locks
				.acquire(&StorageLock::write_keys([Bytes::from(format!("key-{i}"))]))
				.await;
			drop(guard);
		}

		assert_eq!(locks.key_locks.len(), lock_slots);
	}
}
