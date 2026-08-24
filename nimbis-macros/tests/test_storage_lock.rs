use std::sync::Arc;
use std::sync::Mutex;

use nimbis_macros::storage_lock;

#[derive(Clone, Default)]
struct TestStorage {
	events: Arc<Mutex<Vec<String>>>,
}

struct TestGuard {
	events: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone, Copy)]
enum TestDataType {
	String,
	Hash,
}

impl TestDataType {
	fn name(self) -> &'static str {
		match self {
			Self::String => "string",
			Self::Hash => "hash",
		}
	}
}

impl Drop for TestGuard {
	fn drop(&mut self) {
		self.events.lock().unwrap().push("drop".to_string());
	}
}

impl TestStorage {
	async fn read_lock(
		&self,
		data_type: TestDataType,
		keys: impl IntoIterator<Item = String>,
	) -> TestGuard {
		let keys = keys.into_iter().collect::<Vec<_>>().join(",");
		self.events
			.lock()
			.unwrap()
			.push(format!("read:{}:{keys}", data_type.name()));
		TestGuard {
			events: self.events.clone(),
		}
	}

	async fn write_lock(
		&self,
		data_type: TestDataType,
		keys: impl IntoIterator<Item = String>,
	) -> TestGuard {
		let keys = keys.into_iter().collect::<Vec<_>>().join(",");
		self.events
			.lock()
			.unwrap()
			.push(format!("write:{}:{keys}", data_type.name()));
		TestGuard {
			events: self.events.clone(),
		}
	}

	async fn global_write_lock(&self) -> TestGuard {
		self.events.lock().unwrap().push("global_write".to_string());
		TestGuard {
			events: self.events.clone(),
		}
	}

	#[storage_lock(read, key, TestDataType::String)]
	async fn read_one(&self, key: String) -> usize {
		self.events.lock().unwrap().push(format!("body:{key}"));
		7
	}

	#[storage_lock(write_many, keys, data_type)]
	async fn write_many<I>(&self, data_type: TestDataType, keys: I) -> usize
	where
		I: IntoIterator<Item = String>,
	{
		self.events
			.lock()
			.unwrap()
			.push(format!("body:{}", keys.join(",")));
		keys.len()
	}

	#[storage_lock(global_write)]
	async fn global_write(&self) -> usize {
		self.events.lock().unwrap().push("body".to_string());
		3
	}
}

#[tokio::test]
async fn storage_lock_read_wraps_function_body() {
	let storage = TestStorage::default();

	let result = storage.read_one("alpha".to_string()).await;

	assert_eq!(result, 7);
	assert_eq!(
		*storage.events.lock().unwrap(),
		vec!["read:string:alpha", "body:alpha", "drop"],
	);
}

#[tokio::test]
async fn storage_lock_many_collects_keys_before_locking() {
	let storage = TestStorage::default();

	let result = storage
		.write_many(
			TestDataType::Hash,
			["alpha".to_string(), "beta".to_string()],
		)
		.await;

	assert_eq!(result, 2);
	assert_eq!(
		*storage.events.lock().unwrap(),
		vec!["write:hash:alpha,beta", "body:alpha,beta", "drop"],
	);
}

#[tokio::test]
async fn storage_lock_global_write_wraps_function_body() {
	let storage = TestStorage::default();

	let result = storage.global_write().await;

	assert_eq!(result, 3);
	assert_eq!(
		*storage.events.lock().unwrap(),
		vec!["global_write", "body", "drop"],
	);
}
