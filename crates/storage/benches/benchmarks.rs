use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use bytes::Bytes;
use criterion::BatchSize;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use storage::Storage;
use tokio::runtime::Runtime;

fn bench_runtime() -> Runtime {
	Runtime::new().expect("failed to create benchmark runtime")
}

fn fresh_storage(rt: &Runtime, name: &str) -> (Storage, PathBuf) {
	let path = std::env::temp_dir().join(format!(
		"nimbis_storage_bench_{}_{}",
		name,
		ulid::Ulid::new()
	));
	std::fs::create_dir_all(&path).expect("failed to create benchmark directory");
	let storage = rt
		.block_on(Storage::open(&path, None))
		.expect("failed to open storage");
	(storage, path)
}

fn cleanup_path(path: &PathBuf) {
	let _ = std::fs::remove_dir_all(path);
}

fn bench_string_set(c: &mut Criterion) {
	let rt = bench_runtime();
	let (storage, path) = fresh_storage(&rt, "string_set");
	let value = Bytes::from(vec![b'x'; 128]);
	let counter = AtomicU64::new(0);
	let mut group = c.benchmark_group("storage_string");

	group.throughput(Throughput::Elements(1));
	group.bench_function("set_128b", |b| {
		b.iter(|| {
			let idx = counter.fetch_add(1, Ordering::Relaxed);
			let key = Bytes::from(format!("bench:string:set:{idx}"));
			rt.block_on(storage.set(black_box(key), black_box(value.clone())))
				.expect("set should succeed");
		})
	});
	group.finish();

	cleanup_path(&path);
}

fn bench_string_get(c: &mut Criterion) {
	let rt = bench_runtime();
	let (storage, path) = fresh_storage(&rt, "string_get");
	let key = Bytes::from("bench:string:get:key");
	let value = Bytes::from(vec![b'y'; 256]);
	rt.block_on(storage.set(key.clone(), value))
		.expect("failed to seed string key");
	let mut group = c.benchmark_group("storage_string");

	group.throughput(Throughput::Elements(1));
	group.bench_function("get_256b", |b| {
		b.iter(|| {
			rt.block_on(storage.get(black_box(key.clone())))
				.expect("get should succeed")
		})
	});
	group.finish();

	cleanup_path(&path);
}

fn bench_hash_hset(c: &mut Criterion) {
	let rt = bench_runtime();
	let (storage, path) = fresh_storage(&rt, "hash_hset");
	let key = Bytes::from("bench:hash");
	let value = Bytes::from(vec![b'h'; 64]);
	let counter = AtomicU64::new(0);
	let mut group = c.benchmark_group("storage_hash");

	group.throughput(Throughput::Elements(1));
	group.bench_function("hset_new_field", |b| {
		b.iter(|| {
			let idx = counter.fetch_add(1, Ordering::Relaxed);
			let field = Bytes::from(format!("field:{idx}"));
			rt.block_on(storage.hset(
				black_box(key.clone()),
				black_box(field),
				black_box(value.clone()),
			))
			.expect("hset should succeed");
		})
	});
	group.finish();

	cleanup_path(&path);
}

fn bench_list_lrange(c: &mut Criterion) {
	let rt = bench_runtime();
	let (storage, path) = fresh_storage(&rt, "list_lrange");
	let key = Bytes::from("bench:list");
	let elements: Vec<_> = (0..256)
		.map(|i| Bytes::from(format!("item:{i:03}")))
		.collect();
	rt.block_on(storage.rpush(key.clone(), elements))
		.expect("failed to seed list");
	let mut group = c.benchmark_group("storage_list");

	group.throughput(Throughput::Elements(64));
	group.bench_function("lrange_64_items", |b| {
		b.iter(|| {
			rt.block_on(storage.lrange(black_box(key.clone()), 32, 95))
				.expect("lrange should succeed")
		})
	});
	group.finish();

	cleanup_path(&path);
}

fn bench_set_smembers(c: &mut Criterion) {
	let rt = bench_runtime();
	let (storage, path) = fresh_storage(&rt, "set_smembers");
	let key = Bytes::from("bench:set");
	let members: Vec<_> = (0..256)
		.map(|i| Bytes::from(format!("member:{i:03}")))
		.collect();
	rt.block_on(storage.sadd(key.clone(), members))
		.expect("failed to seed set");
	let mut group = c.benchmark_group("storage_set");

	group.throughput(Throughput::Elements(256));
	group.bench_function("smembers_256_items", |b| {
		b.iter(|| {
			rt.block_on(storage.smembers(black_box(key.clone())))
				.expect("smembers should succeed")
		})
	});
	group.finish();

	cleanup_path(&path);
}

fn bench_zset_zadd(c: &mut Criterion) {
	let rt = bench_runtime();
	let (storage, path) = fresh_storage(&rt, "zset_zadd");
	let key = Bytes::from("bench:zset");
	let counter = AtomicU64::new(0);
	let mut group = c.benchmark_group("storage_zset");

	group.throughput(Throughput::Elements(1));
	group.bench_function("zadd_new_member", |b| {
		b.iter(|| {
			let idx = counter.fetch_add(1, Ordering::Relaxed);
			let score = idx as f64;
			let member = Bytes::from(format!("member:{idx}"));
			rt.block_on(storage.zadd(black_box(key.clone()), black_box(vec![(score, member)])))
				.expect("zadd should succeed");
		})
	});
	group.finish();

	cleanup_path(&path);
}

fn bench_storage_open(c: &mut Criterion) {
	let rt = Arc::new(bench_runtime());
	let mut group = c.benchmark_group("storage_open");

	group.throughput(Throughput::Elements(1));
	group.bench_function("open_empty_storage", |b| {
		b.iter_batched(
			|| {
				std::env::temp_dir()
					.join(format!("nimbis_storage_bench_open_{}", ulid::Ulid::new()))
			},
			|path| {
				std::fs::create_dir_all(&path).expect("failed to create benchmark directory");
				let storage = rt
					.block_on(Storage::open(&path, None))
					.expect("open should succeed");
				rt.block_on(storage.close()).expect("close should succeed");
				drop(storage);
				cleanup_path(&path);
			},
			BatchSize::SmallInput,
		)
	});
	group.finish();
}

criterion_group!(
	benches,
	bench_storage_open,
	bench_string_set,
	bench_string_get,
	bench_hash_hset,
	bench_list_lrange,
	bench_set_smembers,
	bench_zset_zadd,
);
criterion_main!(benches);
