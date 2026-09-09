use std::env;
use std::process;
use std::sync::Barrier;
use std::time::Duration;

use fail_parallel::FailPointRegistry;
use futures::FutureExt;
use rstest::rstest;
use slatedb::config::Settings;
use tempfile::tempdir;
use tokio::process::Command;
use tokio::sync::Notify;
use tokio::task::spawn_blocking;
use tokio::time::timeout;

use super::*;

const CASE_ENV: &str = "NIMBIS_RECOVERY_TEST_CASE";
const PATH_ENV: &str = "NIMBIS_RECOVERY_TEST_PATH";
const WAL_FAILPOINT: &str = "write-wal-sst-io-error";
const ABRUPT_EXIT: i32 = 17;

#[rstest]
#[case("close", "after")]
#[case("abrupt_pending", "before")]
#[case("abrupt_durable", "after")]
#[case("delayed_wal", "after")]
#[case("close_io_error", "before")]
#[case("failed_db", "before")]
#[tokio::test]
async fn recovery_contract(#[case] mode: &str, #[case] expected: &str) {
	let directory = tempdir().unwrap();
	let child = Command::new(env::current_exe().unwrap())
		.args([
			"--exact",
			"storage::recovery_tests::recovery_child",
			"--nocapture",
		])
		.env(CASE_ENV, mode)
		.env(PATH_ENV, directory.path())
		.kill_on_drop(true)
		.output();
	let output = timeout(Duration::from_secs(45), child)
		.await
		.expect("recovery subprocess hung")
		.unwrap();
	assert_eq!(
		output.status.code(),
		Some(if mode == "close" { 0 } else { ABRUPT_EXIT }),
		"{mode}: {}\n{}",
		String::from_utf8_lossy(&output.stdout),
		String::from_utf8_lossy(&output.stderr)
	);

	let storage = Storage::open(directory.path(), None).await.unwrap();
	assert_eq!(
		storage.get(Bytes::from("key")).await.unwrap(),
		Some(Bytes::copy_from_slice(expected.as_bytes()))
	);
	if matches!(mode, "close" | "close_io_error" | "failed_db") {
		assert_eq!(
			storage.lrange(Bytes::from("list"), 0, -1).await.unwrap(),
			vec![Bytes::from("sibling-write")],
			"a failed string DB must not cancel the list DB's close"
		);
	}
	storage.close().await.unwrap();
}

// The test executable is also the crash writer. Environment variables are read
// only by this test; production storage settings and command ACKs are unchanged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovery_child() {
	let Ok(mode) = env::var(CASE_ENV) else {
		return;
	};
	let path = env::var_os(PATH_ENV).unwrap();
	let mut storage = Storage::open(&path, None).await.unwrap();
	storage.string_db.raw().close().await.unwrap();
	let failpoints = Arc::new(FailPointRegistry::new());
	let db = Db::builder(
		"string",
		Arc::new(LocalFileSystem::new_with_prefix(&path).unwrap()),
	)
	.with_settings(Settings {
		// Keep pending writes pending until an explicit flush: no timer races.
		flush_interval: None,
		..Settings::default()
	})
	.with_fp_registry(failpoints.clone())
	.build()
	.await
	.unwrap();
	let db = Arc::new(db);
	storage.string_db = TypedDb::new(db.clone(), Arc::new(DefaultMetricsRecorder::new()));
	storage
		.set(Bytes::from("key"), Bytes::from("before"))
		.await
		.unwrap();
	db.flush().await.unwrap();
	storage
		.set(Bytes::from("key"), Bytes::from("after"))
		.await
		.unwrap();
	assert_eq!(
		storage.get(Bytes::from("key")).await.unwrap(),
		Some(Bytes::from("after"))
	);
	let pending = db
		.get_key_value(TopLevelKey::new(Bytes::from("key")).unwrap().encode())
		.await
		.unwrap()
		.unwrap();
	assert!(db.status().durable_seq < pending.seq);
	let barrier = db
		.put(
			TopLevelKey::new(Bytes::from("barrier")).unwrap().encode(),
			StringValue::new(Bytes::new()).encode(),
		)
		.await
		.unwrap();
	assert!(barrier.await_durable().now_or_never().is_none());
	storage
		.rpush(Bytes::from("list"), vec![Bytes::from("sibling-write")])
		.await
		.unwrap();

	match mode.as_str() {
		"close" => storage.close().await.unwrap(),
		"abrupt_pending" => {}
		"abrupt_durable" => {
			db.flush().await.unwrap();
			barrier.await_durable().await.unwrap();
			assert!(db.status().durable_seq >= pending.seq);
		}
		"delayed_wal" => {
			let entered = Arc::new(Notify::new());
			let release = Arc::new(Barrier::new(2));
			let entered_wal = entered.clone();
			let release_wal = release.clone();
			fail_parallel::cfg_callback(failpoints.clone(), WAL_FAILPOINT, move || {
				entered_wal.notify_one();
				release_wal.wait();
			})
			.unwrap();
			let flush = db.clone();
			let flush = tokio::spawn(async move { flush.flush().await });
			timeout(Duration::from_secs(10), entered.notified())
				.await
				.unwrap();
			assert!(db.status().durable_seq < pending.seq);
			assert!(barrier.await_durable().now_or_never().is_none());
			fail_parallel::remove(failpoints.clone(), WAL_FAILPOINT);
			spawn_blocking(move || release.wait()).await.unwrap();
			flush.await.unwrap().unwrap();
			barrier.await_durable().await.unwrap();
		}
		"close_io_error" | "failed_db" => {
			fail_parallel::cfg(
				failpoints.clone(),
				WAL_FAILPOINT,
				if mode == "failed_db" {
					"panic"
				} else {
					"return"
				},
			)
			.unwrap();
			if mode == "failed_db" {
				assert!(db.flush().await.is_err());
				db.subscribe()
					.wait_for(|status| status.close_reason.is_some())
					.await
					.unwrap();
				assert!(barrier.await_durable().await.is_err());
			}
			assert!(storage.close().await.is_err());
			assert!(
				storage
					.all_raw_dbs()
					.iter()
					.all(|(_, db)| db.status().close_reason.is_some())
			);
		}
		_ => panic!("unknown recovery case: {mode}"),
	}
	if mode != "close" {
		// Do not drop Storage or the Tokio runtime: this is a real abrupt exit.
		process::exit(ABRUPT_EXIT);
	}
}
