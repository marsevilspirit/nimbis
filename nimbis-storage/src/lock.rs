use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use dashmap::DashMap;
use tokio::sync::OwnedRwLockReadGuard;
use tokio::sync::OwnedRwLockWriteGuard;
use tokio::sync::RwLock;

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

#[derive(Debug, Default)]
pub struct StorageLocks {
	db_lock: Arc<RwLock<()>>,
	// Keep per-key lock objects stable for the storage lifetime. Removing them
	// after release can race with a new acquirer and split one raw key across two locks.
	key_locks: Arc<DashMap<Bytes, Arc<RwLock<()>>>>,
}

impl StorageLocks {
	pub fn new() -> Self {
		Self::default()
	}

	pub async fn acquire(&self, lock: &StorageLock) -> StorageLockGuard {
		match lock.mode {
			StorageLockMode::None => StorageLockGuard::default(),
			StorageLockMode::GlobalWrite => StorageLockGuard {
				db_read_guard: None,
				db_write_guard: Some(self.db_lock.clone().write_owned().await),
				key_guards: Vec::new(),
			},
			StorageLockMode::Keys => self.acquire_key_locks(lock).await,
		}
	}

	async fn acquire_key_locks(&self, lock: &StorageLock) -> StorageLockGuard {
		let db_read_guard = self.db_lock.clone().read_owned().await;
		let key_modes = ordered_key_modes(lock);
		let mut key_guards = Vec::with_capacity(key_modes.len());

		for (key, mode) in key_modes {
			let lock = self
				.key_locks
				.entry(key.clone())
				.or_insert_with(|| Arc::new(RwLock::new(())))
				.clone();
			match mode {
				KeyMode::Read => key_guards.push(KeyLockGuard::Read(lock.read_owned().await)),
				KeyMode::Write => key_guards.push(KeyLockGuard::Write(lock.write_owned().await)),
			}
		}

		StorageLockGuard {
			db_read_guard: Some(db_read_guard),
			db_write_guard: None,
			key_guards,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyMode {
	Read,
	Write,
}

fn ordered_key_modes(lock: &StorageLock) -> Vec<(Bytes, KeyMode)> {
	let mut keys = BTreeMap::new();
	for key in &lock.read_keys {
		keys.entry(key.clone()).or_insert(KeyMode::Read);
	}
	for key in &lock.write_keys {
		keys.insert(key.clone(), KeyMode::Write);
	}
	keys.into_iter().collect()
}

#[derive(Default)]
pub struct StorageLockGuard {
	db_read_guard: Option<OwnedRwLockReadGuard<()>>,
	db_write_guard: Option<OwnedRwLockWriteGuard<()>>,
	key_guards: Vec<KeyLockGuard>,
}

impl Drop for StorageLockGuard {
	fn drop(&mut self) {
		let _ = self.db_read_guard.as_ref();
		let _ = self.db_write_guard.as_ref();
		for guard in &self.key_guards {
			match guard {
				KeyLockGuard::Read(guard) => {
					let _ = guard;
				}
				KeyLockGuard::Write(guard) => {
					let _ = guard;
				}
			}
		}
		self.key_guards.clear();
	}
}

enum KeyLockGuard {
	Read(OwnedRwLockReadGuard<()>),
	Write(OwnedRwLockWriteGuard<()>),
}
