use std::future::Future;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use bytes::Bytes;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use nimbis_storage::Storage;
use nimbis_storage::error::StorageError;
use tokio::runtime::Runtime;

fn bench_runtime() -> Runtime {
	Runtime::new().expect("failed to create benchmark runtime")
}

fn bench_path(name: &str) -> PathBuf {
	std::env::temp_dir().join(format!(
		"nimbis_storage_bench_{}_{}",
		name,
		ulid::Ulid::new()
	))
}

struct BenchStore {
	rt: Runtime,
	storage: Storage,
	path: PathBuf,
}

impl BenchStore {
	fn open(name: &str) -> Self {
		let rt = bench_runtime();
		let path = bench_path(name);
		let storage = rt
			.block_on(Storage::open(&path, None))
			.expect("failed to open storage");

		Self { rt, storage, path }
	}

	fn run<T>(&self, future: impl Future<Output = Result<T, StorageError>>, message: &str) -> T {
		self.rt.block_on(future).expect(message)
	}

	fn close(self) {
		let Self { rt, storage, path } = self;
		rt.block_on(storage.close())
			.expect("failed to close storage");
		drop(storage);
		std::fs::remove_dir_all(&path).expect("failed to remove benchmark directory");
	}
}

fn bench_string_set(c: &mut Criterion) {
	let bench = BenchStore::open("string_set");
	let value = Bytes::from(vec![b'x'; 128]);
	let mut next_key = 0;
	let mut group = c.benchmark_group("storage_string");

	group.throughput(Throughput::Elements(1));
	group.bench_function("set_128b", |b| {
		b.iter(|| {
			let key = Bytes::from(format!("bench:string:set:{next_key}"));
			next_key += 1;
			bench.run(
				bench.storage.set(black_box(key), black_box(value.clone())),
				"set should succeed",
			);
		})
	});
	group.finish();

	bench.close();
}

fn bench_string_get(c: &mut Criterion) {
	let bench = BenchStore::open("string_get");
	let key = Bytes::from("bench:string:get:key");
	let value = Bytes::from(vec![b'y'; 256]);
	bench.run(
		bench.storage.set(key.clone(), value),
		"failed to seed string key",
	);
	let mut group = c.benchmark_group("storage_string");

	group.throughput(Throughput::Elements(1));
	group.bench_function("get_256b", |b| {
		b.iter(|| {
			bench.run(
				bench.storage.get(black_box(key.clone())),
				"get should succeed",
			)
		})
	});
	group.finish();

	bench.close();
}

fn bench_hash_hset(c: &mut Criterion) {
	let bench = BenchStore::open("hash_hset");
	let key = Bytes::from("bench:hash");
	let value = Bytes::from(vec![b'h'; 64]);
	let mut next_field = 0;
	let mut group = c.benchmark_group("storage_hash");

	group.throughput(Throughput::Elements(1));
	group.bench_function("hset_new_field", |b| {
		b.iter(|| {
			let field = Bytes::from(format!("field:{next_field}"));
			next_field += 1;
			bench.run(
				bench.storage.hset(
					black_box(key.clone()),
					black_box(field),
					black_box(value.clone()),
				),
				"hset should succeed",
			);
		})
	});
	group.finish();

	bench.close();
}

fn bench_list_lrange(c: &mut Criterion) {
	let bench = BenchStore::open("list_lrange");
	let key = Bytes::from("bench:list");
	let elements: Vec<_> = (0..256)
		.map(|i| Bytes::from(format!("item:{i:03}")))
		.collect();
	bench.run(
		bench.storage.rpush(key.clone(), elements),
		"failed to seed list",
	);
	let mut group = c.benchmark_group("storage_list");

	group.throughput(Throughput::Elements(64));
	group.bench_function("lrange_64_items", |b| {
		b.iter(|| {
			bench.run(
				bench.storage.lrange(black_box(key.clone()), 32, 95),
				"lrange should succeed",
			)
		})
	});
	group.finish();

	bench.close();
}

fn bench_set_smembers(c: &mut Criterion) {
	let bench = BenchStore::open("set_smembers");
	let key = Bytes::from("bench:set");
	let members: Vec<_> = (0..256)
		.map(|i| Bytes::from(format!("member:{i:03}")))
		.collect();
	bench.run(
		bench.storage.sadd(key.clone(), members),
		"failed to seed set",
	);
	let mut group = c.benchmark_group("storage_set");

	group.throughput(Throughput::Elements(256));
	group.bench_function("smembers_256_items", |b| {
		b.iter(|| {
			bench.run(
				bench.storage.smembers(black_box(key.clone())),
				"smembers should succeed",
			)
		})
	});
	group.finish();

	bench.close();
}

fn bench_zset_zadd(c: &mut Criterion) {
	let bench = BenchStore::open("zset_zadd");
	let key = Bytes::from("bench:zset");
	let mut next_member = 0;
	let mut group = c.benchmark_group("storage_zset");

	group.throughput(Throughput::Elements(1));
	group.bench_function("zadd_new_member", |b| {
		b.iter(|| {
			let score = next_member as f64;
			let member = Bytes::from(format!("member:{next_member}"));
			next_member += 1;
			bench.run(
				bench
					.storage
					.zadd(black_box(key.clone()), black_box(vec![(score, member)])),
				"zadd should succeed",
			);
		})
	});
	group.finish();

	bench.close();
}

fn bench_storage_open(c: &mut Criterion) {
	let rt = bench_runtime();
	let mut group = c.benchmark_group("storage_open");

	group.throughput(Throughput::Elements(1));
	group.bench_function("open_empty_storage", |b| {
		b.iter_custom(|iters| {
			let mut total = Duration::ZERO;
			for _ in 0..iters {
				let path = bench_path("open");
				let start = Instant::now();
				let storage = rt
					.block_on(Storage::open(&path, None))
					.expect("open should succeed");
				total += start.elapsed();
				rt.block_on(storage.close()).expect("close should succeed");
				drop(storage);
				std::fs::remove_dir_all(&path).expect("failed to remove benchmark directory");
			}
			total
		})
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
