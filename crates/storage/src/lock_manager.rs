use std::sync::Arc;

use bytes::Bytes;
use dashmap::DashMap;
use tokio::sync::Mutex;
use tokio::sync::OwnedMutexGuard;

/// Lock manager for key-level locking
///
/// This provides fine-grained concurrency control at the key level,
/// allowing multiple commands to execute concurrently as long as they
/// operate on different keys.
pub struct LockManager {
	locks: DashMap<Bytes, Arc<Mutex<()>>>,
}

impl LockManager {
	/// Create a new lock manager
	pub fn new() -> Self {
		Self {
			locks: DashMap::new(),
		}
	}

	/// Acquire a lock for a single key
	///
	/// The lock is held until the returned guard is dropped.
	pub async fn lock(&self, key: &Bytes) -> LockGuard {
		let mutex = self.get_or_create_lock(key);
		let guard = mutex.lock_owned().await;
		LockGuard { _guard: guard }
	}

	/// Acquire locks for multiple keys
	///
	/// Keys are sorted to ensure consistent lock ordering and prevent deadlocks.
	/// The locks are held until the returned guard is dropped.
	pub async fn multi_lock(&self, keys: &[Bytes]) -> MultiLockGuard {
		// Sort keys to ensure consistent lock ordering (prevents deadlock)
		let mut sorted_keys: Vec<_> = keys.to_vec();
		sorted_keys.sort();
		sorted_keys.dedup(); // Remove duplicates

		let mut guards = Vec::with_capacity(sorted_keys.len());
		for key in &sorted_keys {
			let mutex = self.get_or_create_lock(key);
			let guard = mutex.lock_owned().await;
			guards.push(guard);
		}

		MultiLockGuard { _guards: guards }
	}

	/// Get existing lock or create a new one
	fn get_or_create_lock(&self, key: &Bytes) -> Arc<Mutex<()>> {
		self.locks
			.entry(key.clone())
			.or_insert_with(|| Arc::new(Mutex::new(())))
			.clone()
	}

	/// Clean up unused locks (called periodically or when memory pressure is high)
	///
	/// This removes locks that are not currently held and have no other references.
	pub fn cleanup_unused_locks(&self) {
		self.locks.retain(|_, mutex| Arc::strong_count(mutex) > 1);
	}
}

impl Default for LockManager {
	fn default() -> Self {
		Self::new()
	}
}

/// RAII guard for a single key lock
///
/// The lock is automatically released when this guard is dropped.
pub struct LockGuard {
	_guard: OwnedMutexGuard<()>,
}

/// RAII guard for multiple key locks
///
/// The locks are automatically released when this guard is dropped.
pub struct MultiLockGuard {
	_guards: Vec<OwnedMutexGuard<()>>,
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use tokio::time::timeout;

	use super::*;

	#[tokio::test]
	async fn test_single_lock() {
		let manager = LockManager::new();
		let key = Bytes::from("test_key");

		// Acquire lock
		let _guard = manager.lock(&key).await;

		// Lock should be held
		assert!(
			timeout(Duration::from_millis(10), manager.lock(&key))
				.await
				.is_err()
		);

		// Drop guard
		drop(_guard);

		// Lock should be available again
		let _guard2 = timeout(Duration::from_millis(10), manager.lock(&key))
			.await
			.expect("should acquire lock");
	}

	#[tokio::test]
	async fn test_multi_lock() {
		let manager = LockManager::new();
		let key1 = Bytes::from("key1");
		let key2 = Bytes::from("key2");

		// Acquire locks
		let _guard = manager.multi_lock(&[key1.clone(), key2.clone()]).await;

		// Both locks should be held
		assert!(
			timeout(Duration::from_millis(10), manager.lock(&key1))
				.await
				.is_err()
		);
		assert!(
			timeout(Duration::from_millis(10), manager.lock(&key2))
				.await
				.is_err()
		);

		// Drop guard
		drop(_guard);

		// Both locks should be available
		let _guard1 = timeout(Duration::from_millis(10), manager.lock(&key1))
			.await
			.expect("should acquire lock");
		let _guard2 = timeout(Duration::from_millis(10), manager.lock(&key2))
			.await
			.expect("should acquire lock");
	}

	#[tokio::test]
	async fn test_lock_ordering() {
		let manager = LockManager::new();
		let key1 = Bytes::from("aaa");
		let key2 = Bytes::from("zzz");

		// Test that locks are acquired in sorted order regardless of input order
		let _guard1 = manager.multi_lock(&[key2.clone(), key1.clone()]).await;
		drop(_guard1);
		let _guard2 = manager.multi_lock(&[key1.clone(), key2.clone()]).await;
		drop(_guard2);

		// If ordering works correctly, this shouldn't deadlock
	}

	#[tokio::test]
	async fn test_cleanup() {
		let manager = LockManager::new();
		let key = Bytes::from("test_key");

		{
			let _guard = manager.lock(&key).await;
			assert_eq!(manager.locks.len(), 1);
		}

		// Lock should still exist (weak reference)
		assert_eq!(manager.locks.len(), 1);

		// Clean up
		manager.cleanup_unused_locks();

		// Lock should be removed since it's not in use
		assert_eq!(manager.locks.len(), 0);
	}

	#[tokio::test]
	async fn test_concurrent_different_keys() {
		let manager = Arc::new(LockManager::new());
		let key1 = Bytes::from("key1");
		let key2 = Bytes::from("key2");

		let manager1 = manager.clone();
		let k1 = key1.clone();
		let handle1 = tokio::spawn(async move {
			let _guard = manager1.lock(&k1).await;
			tokio::time::sleep(Duration::from_millis(50)).await;
		});

		let manager2 = manager.clone();
		let k2 = key2.clone();
		let handle2 = tokio::spawn(async move {
			let _guard = manager2.lock(&k2).await;
			tokio::time::sleep(Duration::from_millis(50)).await;
		});

		// Both should complete without blocking each other
		let result = timeout(Duration::from_millis(100), async {
			handle1.await.unwrap();
			handle2.await.unwrap();
		})
		.await;

		assert!(result.is_ok(), "Different keys should not block each other");
	}
}
