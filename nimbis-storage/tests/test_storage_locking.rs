use std::sync::Arc;

use bytes::Bytes;
use nimbis_storage::Storage;

async fn get_storage() -> (Storage, std::path::PathBuf) {
	let timestamp = ulid::Ulid::new().to_string();
	let path = std::env::temp_dir().join(format!("nimbis_test_storage_locking_{}", timestamp));
	std::fs::create_dir_all(&path).unwrap();
	let storage = Storage::open(&path, None).await.unwrap();
	(storage, path)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn storage_incr_serializes_same_key_writes() {
	let (storage, path) = get_storage().await;
	let storage = Arc::new(storage);
	let key = Bytes::from_static(b"counter");
	let workers = 16;
	let increments_per_worker = 100;

	storage
		.set(key.clone(), Bytes::from_static(b"0"))
		.await
		.unwrap();

	let mut tasks = Vec::new();
	for _ in 0..workers {
		let storage = storage.clone();
		let key = key.clone();
		tasks.push(tokio::spawn(async move {
			for _ in 0..increments_per_worker {
				storage.incr(key.clone()).await.unwrap();
			}
		}));
	}

	for task in tasks {
		task.await.unwrap();
	}

	let stored = storage.get(key).await.unwrap().unwrap();
	assert_eq!(
		stored,
		Bytes::from((workers * increments_per_worker).to_string())
	);

	let _ = std::fs::remove_dir_all(path);
}
