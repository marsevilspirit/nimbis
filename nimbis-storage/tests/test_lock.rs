use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use nimbis_storage::lock::StorageLock;
use nimbis_storage::lock::StorageLockMode;
use nimbis_storage::lock::StorageLocks;
use tokio::sync::Barrier;

#[tokio::test]
async fn read_locks_can_overlap_for_the_same_key() {
	let locks = StorageLocks::new();
	let first = locks
		.acquire(&StorageLock::read_keys([Bytes::from_static(b"key")]))
		.await;
	let second = tokio::time::timeout(
		Duration::from_millis(50),
		locks.acquire(&StorageLock::read_keys([Bytes::from_static(b"key")])),
	)
	.await;

	drop(first);
	assert!(second.is_ok(), "read/read locking should not block");
}

#[tokio::test]
async fn write_lock_excludes_same_key_readers() {
	let locks = StorageLocks::new();
	let write_guard = locks
		.acquire(&StorageLock::write_keys([Bytes::from_static(b"key")]))
		.await;

	let blocked = tokio::time::timeout(
		Duration::from_millis(50),
		locks.acquire(&StorageLock::read_keys([Bytes::from_static(b"key")])),
	)
	.await;

	drop(write_guard);
	assert!(blocked.is_err(), "write lock should block same-key readers");
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
				.acquire(&StorageLock::write_keys([
					Bytes::from_static(b"a"),
					Bytes::from_static(b"b"),
				]))
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
				.acquire(&StorageLock {
					mode: StorageLockMode::Keys,
					read_keys: Vec::new(),
					write_keys: vec![Bytes::from_static(b"b"), Bytes::from_static(b"a")],
				})
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
		locks.acquire(&StorageLock::write_keys([Bytes::from_static(b"key")])),
	)
	.await;

	drop(global_guard);
	assert!(blocked.is_err(), "global write should block key locks");
}
