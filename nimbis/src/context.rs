use std::sync::Arc;
use std::sync::OnceLock;

use bytes::Bytes;
use dashmap::DashMap;
use tokio::sync::Mutex;
use tokio::sync::OwnedMutexGuard;

use crate::client::ClientSessions;

#[derive(Debug)]
pub struct GlobalContext {
	pub client_sessions: Arc<ClientSessions>,
	pub key_locks: Arc<KeyLockManager>,
}

impl GlobalContext {
	pub fn new(client_sessions: Arc<ClientSessions>) -> Self {
		Self {
			client_sessions,
			key_locks: Arc::new(KeyLockManager::default()),
		}
	}
}

#[derive(Debug, Default)]
pub struct KeyLockManager {
	locks: DashMap<Bytes, Arc<Mutex<()>>>,
}

#[derive(Debug)]
pub struct MultiKeyGuard {
	_guards: Vec<OwnedMutexGuard<()>>,
}

impl KeyLockManager {
	pub async fn lock_keys(&self, keys: &[Bytes]) -> MultiKeyGuard {
		let mut keys = keys.to_vec();
		keys.sort();
		keys.dedup();

		let mut guards = Vec::with_capacity(keys.len());
		for key in keys {
			let lock = self
				.locks
				.entry(key)
				.or_insert_with(|| Arc::new(Mutex::new(())))
				.clone();
			guards.push(lock.lock_owned().await);
		}

		MultiKeyGuard { _guards: guards }
	}
}

pub static GCTX: OnceLock<GlobalContext> = OnceLock::new();

pub fn init_global_context(client_sessions: Arc<ClientSessions>) {
	let _ = GCTX.set(GlobalContext::new(client_sessions));
}

#[macro_export]
macro_rules! GCTX {
	($field:ident) => {
		&$crate::context::GCTX
			.get()
			.expect("GlobalContext is not initialized")
			.$field
	};
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::sync::atomic::AtomicUsize;
	use std::sync::atomic::Ordering;
	use std::time::Duration;

	use bytes::Bytes;

	use super::KeyLockManager;

	#[tokio::test]
	async fn key_lock_manager_deduplicates_repeated_keys() {
		let locks = KeyLockManager::default();
		let _guard = locks
			.lock_keys(&[
				Bytes::from_static(b"k1"),
				Bytes::from_static(b"k1"),
				Bytes::from_static(b"k2"),
			])
			.await;
	}

	#[tokio::test]
	async fn key_lock_manager_serializes_overlapping_keys() {
		let locks = Arc::new(KeyLockManager::default());
		let entered = Arc::new(AtomicUsize::new(0));
		let max_seen = Arc::new(AtomicUsize::new(0));
		let key = Bytes::from_static(b"shared");

		let mut tasks = Vec::new();
		for _ in 0..8 {
			let locks = locks.clone();
			let entered = entered.clone();
			let max_seen = max_seen.clone();
			let key = key.clone();
			tasks.push(tokio::spawn(async move {
				let _guard = locks.lock_keys(&[key]).await;
				let current = entered.fetch_add(1, Ordering::SeqCst) + 1;
				max_seen.fetch_max(current, Ordering::SeqCst);
				tokio::time::sleep(Duration::from_millis(2)).await;
				entered.fetch_sub(1, Ordering::SeqCst);
			}));
		}

		for task in tasks {
			task.await.expect("lock task should complete");
		}
		assert_eq!(max_seen.load(Ordering::SeqCst), 1);
	}

	#[tokio::test]
	async fn key_lock_manager_allows_disjoint_keys() {
		let locks = Arc::new(KeyLockManager::default());
		let entered = Arc::new(AtomicUsize::new(0));
		let max_seen = Arc::new(AtomicUsize::new(0));

		let mut tasks = Vec::new();
		for idx in 0..8 {
			let locks = locks.clone();
			let entered = entered.clone();
			let max_seen = max_seen.clone();
			let key = Bytes::from(format!("key:{idx}"));
			tasks.push(tokio::spawn(async move {
				let _guard = locks.lock_keys(&[key]).await;
				let current = entered.fetch_add(1, Ordering::SeqCst) + 1;
				max_seen.fetch_max(current, Ordering::SeqCst);
				tokio::time::sleep(Duration::from_millis(2)).await;
				entered.fetch_sub(1, Ordering::SeqCst);
			}));
		}

		for task in tasks {
			task.await.expect("lock task should complete");
		}
		assert!(max_seen.load(Ordering::SeqCst) > 1);
	}
}
