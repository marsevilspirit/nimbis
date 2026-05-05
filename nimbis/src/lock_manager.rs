use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use bytes::Bytes;
use dashmap::DashMap;
use tokio::sync::Mutex;
use tokio::sync::OwnedMutexGuard;

#[derive(Debug, Default)]
pub struct KeyLockManager {
    locks: DashMap<Bytes, Arc<KeyLockEntry>>,
}

#[derive(Debug)]
struct KeyLockEntry {
    mutex: Arc<Mutex<()>>,
    refs: AtomicUsize,
}

#[derive(Debug)]
pub struct MultiKeyGuard<'a> {
    manager: &'a KeyLockManager,
    locked_keys: Vec<(Bytes, Arc<KeyLockEntry>)>,
    guards: Vec<OwnedMutexGuard<()>>,
}

impl KeyLockManager {
    pub async fn lock_keys(&self, keys: &[Bytes]) -> MultiKeyGuard<'_> {
        let mut keys = keys.to_vec();
        keys.sort();
        keys.dedup();

        let mut guards = Vec::with_capacity(keys.len());
        let mut locked_keys = Vec::with_capacity(keys.len());
        for key in keys {
            let entry = {
                let entry_ref = self.locks.entry(key.clone()).or_insert_with(|| {
                    Arc::new(KeyLockEntry {
                        mutex: Arc::new(Mutex::new(())),
                        refs: AtomicUsize::new(0),
                    })
                });
                entry_ref.refs.fetch_add(1, Ordering::SeqCst);
                entry_ref.clone()
            };
            let guard = entry.mutex.clone().lock_owned().await;
            guards.push(guard);
            locked_keys.push((key, entry));
        }

        MultiKeyGuard {
            manager: self,
            locked_keys,
            guards,
        }
    }
}

impl Drop for MultiKeyGuard<'_> {
    fn drop(&mut self) {
        drop(std::mem::take(&mut self.guards));

        for (key, entry) in self.locked_keys.drain(..) {
            if entry.refs.fetch_sub(1, Ordering::SeqCst) == 1 {
                self.manager.locks.remove_if(&key, |_, current| {
                    Arc::ptr_eq(current, &entry) && entry.refs.load(Ordering::SeqCst) == 0
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use bytes::Bytes;

    use super::KeyLockManager;

    #[tokio::test]
    async fn key_lock_manager_deduplicates_repeated_keys() {
        let locks = KeyLockManager::default();
        {
            let _guard = locks
                .lock_keys(&[
                    Bytes::from_static(b"k1"),
                    Bytes::from_static(b"k1"),
                    Bytes::from_static(b"k2"),
                ])
                .await;
            assert_eq!(locks.locks.len(), 2);
        }
        assert!(locks.locks.is_empty());
    }

    #[tokio::test]
    async fn key_lock_manager_removes_idle_locks() {
        let locks = KeyLockManager::default();
        for idx in 0..16 {
            let key = Bytes::from(format!("temporary:{idx}"));
            let _guard = locks.lock_keys(&[key]).await;
            assert_eq!(locks.locks.len(), 1);
        }
        assert!(locks.locks.is_empty());
    }

    #[tokio::test]
    async fn key_lock_manager_keeps_locks_with_waiters() {
        let locks = std::sync::Arc::new(KeyLockManager::default());
        let key = Bytes::from_static(b"shared-waiter");
        let guard = locks.lock_keys(std::slice::from_ref(&key)).await;

        let waiter_locks = locks.clone();
        let waiter_key = key.clone();
        let waiter = tokio::spawn(async move {
            let _guard = waiter_locks.lock_keys(&[waiter_key]).await;
        });

        tokio::time::sleep(Duration::from_millis(2)).await;
        assert_eq!(locks.locks.len(), 1);
        drop(guard);
        waiter.await.expect("waiter should complete");
        assert!(locks.locks.is_empty());
    }

    #[tokio::test]
    async fn key_lock_manager_serializes_overlapping_keys() {
        let locks = std::sync::Arc::new(KeyLockManager::default());
        let entered = std::sync::Arc::new(AtomicUsize::new(0));
        let max_seen = std::sync::Arc::new(AtomicUsize::new(0));
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
        assert!(locks.locks.is_empty());
    }

    #[tokio::test]
    async fn key_lock_manager_allows_disjoint_keys() {
        let locks = std::sync::Arc::new(KeyLockManager::default());
        let entered = std::sync::Arc::new(AtomicUsize::new(0));
        let max_seen = std::sync::Arc::new(AtomicUsize::new(0));

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
        assert!(locks.locks.is_empty());
    }
}