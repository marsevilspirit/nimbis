//! Performance benchmarks for RESP parser
use std::hint::black_box;

use bytes::BytesMut;
use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;

fn bench_parse_simple_string(c: &mut Criterion) {
	let mut group = c.benchmark_group("parse_simple_string");
	let data = b"+OK\r\n";

	group.throughput(Throughput::Bytes(data.len() as u64));
	group.bench_function("simple_string", |b| {
		b.iter(|| {
			let mut buf = BytesMut::from(&data[..]);
			resp::parse(black_box(&mut buf)).unwrap()
		})
	});
	group.finish();
}

fn bench_parse_bulk_string(c: &mut Criterion) {
	let mut group = c.benchmark_group("parse_bulk_string");
	let data = b"$11\r\nhello world\r\n";

	group.throughput(Throughput::Bytes(data.len() as u64));
	group.bench_function("bulk_string", |b| {
		b.iter(|| {
			let mut buf = BytesMut::from(&data[..]);
			resp::parse(black_box(&mut buf)).unwrap()
		})
	});
	group.finish();
}

fn bench_parse_integer(c: &mut Criterion) {
	let mut group = c.benchmark_group("parse_integer");
	let data = b":1000\r\n";

	group.throughput(Throughput::Bytes(data.len() as u64));
	group.bench_function("integer", |b| {
		b.iter(|| {
			let mut buf = BytesMut::from(&data[..]);
			resp::parse(black_box(&mut buf)).unwrap()
		})
	});
	group.finish();
}

fn bench_parse_array(c: &mut Criterion) {
	let mut group = c.benchmark_group("parse_array");
	let data = b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n";

	group.throughput(Throughput::Bytes(data.len() as u64));
	group.bench_function("array_set_command", |b| {
		b.iter(|| {
			let mut buf = BytesMut::from(&data[..]);
			resp::parse(black_box(&mut buf)).unwrap()
		})
	});
	group.finish();
}

fn bench_parse_large_array(c: &mut Criterion) {
	let mut group = c.benchmark_group("parse_large_array");

	// Create array with 100 elements
	let mut data = BytesMut::from("*100\r\n");
	for i in 0..100 {
		let item = format!("$3\r\n{:03}\r\n", i);
		data.extend_from_slice(item.as_bytes());
	}

	group.throughput(Throughput::Bytes(data.len() as u64));
	group.bench_function("array_100_items", |b| {
		b.iter(|| {
			let mut buf = data.clone();
			resp::parse(black_box(&mut buf)).unwrap()
		})
	});
	group.finish();
}

criterion_group!(
	benches,
	bench_parse_simple_string,
	bench_parse_bulk_string,
	bench_parse_integer,
	bench_parse_array,
	bench_parse_large_array,
);

criterion_main!(benches);
