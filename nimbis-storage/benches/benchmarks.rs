use std::future::Future;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use bytes::Bytes;
use criterion::BatchSize;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use nimbis_storage::Storage;
use nimbis_storage::error::StorageError;
use tokio::runtime::Runtime;

const COLLECTION_BATCH_SIZE: usize = 8;

fn bench_runtime() -> Runtime {
	Runtime::new().expect("failed to create benchmark runtime")
}

fn bench_path(name: &str) -> PathBuf {
	std::env::temp_dir().join(format!(
		"nimbis_storage_bench_{}_{}",
		name,
		ulid::Ulid::generate()
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
	let mut next_new_key = 0;
	let mut next_delete_key = 0;
	let delete_fields: Vec<_> = (0..COLLECTION_BATCH_SIZE)
		.map(|i| Bytes::from(format!("delete-field:{i}")))
		.collect();
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
	group.bench_function("hset_new_key_1_field", |b| {
		b.iter_batched(
			|| {
				let key = Bytes::from(format!("bench:hash:new:{next_new_key}"));
				next_new_key += 1;
				(key, Bytes::from("field"), value.clone())
			},
			|(key, field, value)| {
				bench.run(
					bench
						.storage
						.hset(black_box(key), black_box(field), black_box(value)),
					"hset should create a new hash key",
				)
			},
			BatchSize::SmallInput,
		)
	});
	group.throughput(Throughput::Elements(COLLECTION_BATCH_SIZE as u64));
	group.bench_function("hdel_8_fields", |b| {
		b.iter_batched(
			|| {
				let key = Bytes::from(format!("bench:hash:delete:{next_delete_key}"));
				next_delete_key += 1;
				for field in &delete_fields {
					bench.run(
						bench
							.storage
							.hset(key.clone(), field.clone(), value.clone()),
						"failed to seed hash field",
					);
				}
				key
			},
			|key| {
				bench.run(
					bench
						.storage
						.hdel(black_box(key), black_box(delete_fields.as_slice())),
					"hdel should remove seeded fields",
				)
			},
			BatchSize::SmallInput,
		)
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
	let batch_elements: Vec<_> = (0..COLLECTION_BATCH_SIZE)
		.map(|i| Bytes::from(format!("batch-item:{i}")))
		.collect();
	let mut next_push_key = 0;
	let mut next_pop_key = 0;
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
	group.throughput(Throughput::Elements(COLLECTION_BATCH_SIZE as u64));
	group.bench_function("rpush_new_key_8_items", |b| {
		b.iter_batched(
			|| {
				let key = Bytes::from(format!("bench:list:push:{next_push_key}"));
				next_push_key += 1;
				(key, batch_elements.clone())
			},
			|(key, elements)| {
				bench.run(
					bench.storage.rpush(black_box(key), black_box(elements)),
					"rpush should create a new list key",
				)
			},
			BatchSize::SmallInput,
		)
	});
	group.bench_function("lpop_8_items", |b| {
		b.iter_batched(
			|| {
				let key = Bytes::from(format!("bench:list:pop:{next_pop_key}"));
				next_pop_key += 1;
				bench.run(
					bench.storage.rpush(key.clone(), batch_elements.clone()),
					"failed to seed list for lpop",
				);
				key
			},
			|key| {
				bench.run(
					bench
						.storage
						.lpop(black_box(key), black_box(Some(COLLECTION_BATCH_SIZE))),
					"lpop should remove seeded items",
				)
			},
			BatchSize::SmallInput,
		)
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
	let batch_members: Vec<_> = (0..COLLECTION_BATCH_SIZE)
		.map(|i| Bytes::from(format!("batch-member:{i}")))
		.collect();
	let mut next_add_key = 0;
	let mut next_remove_key = 0;
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
	group.throughput(Throughput::Elements(COLLECTION_BATCH_SIZE as u64));
	group.bench_function("sadd_new_key_8_members", |b| {
		b.iter_batched(
			|| {
				let key = Bytes::from(format!("bench:set:add:{next_add_key}"));
				next_add_key += 1;
				(key, batch_members.clone())
			},
			|(key, members)| {
				bench.run(
					bench.storage.sadd(black_box(key), black_box(members)),
					"sadd should create a new set key",
				)
			},
			BatchSize::SmallInput,
		)
	});
	group.bench_function("srem_8_members", |b| {
		b.iter_batched(
			|| {
				let key = Bytes::from(format!("bench:set:remove:{next_remove_key}"));
				next_remove_key += 1;
				bench.run(
					bench.storage.sadd(key.clone(), batch_members.clone()),
					"failed to seed set for srem",
				);
				(key, batch_members.clone())
			},
			|(key, members)| {
				bench.run(
					bench.storage.srem(black_box(key), black_box(members)),
					"srem should remove seeded members",
				)
			},
			BatchSize::SmallInput,
		)
	});
	group.finish();

	bench.close();
}

fn bench_zset_zadd(c: &mut Criterion) {
	let bench = BenchStore::open("zset_zadd");
	let key = Bytes::from("bench:zset");
	let mut next_member = 0;
	let batch_elements: Vec<_> = (0..COLLECTION_BATCH_SIZE)
		.map(|i| (i as f64, Bytes::from(format!("batch-member:{i}"))))
		.collect();
	let batch_members: Vec<_> = batch_elements
		.iter()
		.map(|(_, member)| member.clone())
		.collect();
	let mut next_add_key = 0;
	let mut next_remove_key = 0;
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
	group.throughput(Throughput::Elements(COLLECTION_BATCH_SIZE as u64));
	group.bench_function("zadd_new_key_8_members", |b| {
		b.iter_batched(
			|| {
				let key = Bytes::from(format!("bench:zset:add:{next_add_key}"));
				next_add_key += 1;
				(key, batch_elements.clone())
			},
			|(key, elements)| {
				bench.run(
					bench.storage.zadd(black_box(key), black_box(elements)),
					"zadd should create a new sorted-set key",
				)
			},
			BatchSize::SmallInput,
		)
	});
	group.bench_function("zrem_8_members", |b| {
		b.iter_batched(
			|| {
				let key = Bytes::from(format!("bench:zset:remove:{next_remove_key}"));
				next_remove_key += 1;
				bench.run(
					bench.storage.zadd(key.clone(), batch_elements.clone()),
					"failed to seed sorted set for zrem",
				);
				(key, batch_members.clone())
			},
			|(key, members)| {
				bench.run(
					bench.storage.zrem(black_box(key), black_box(members)),
					"zrem should remove seeded members",
				)
			},
			BatchSize::SmallInput,
		)
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
