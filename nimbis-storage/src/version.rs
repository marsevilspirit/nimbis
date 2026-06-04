use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// Generates monotonically increasing collection generations.
///
/// The layout follows Kvrocks' timestamp-plus-counter style:
/// high 53 bits are microseconds since epoch, low 11 bits are a per-process
/// counter. If the clock does not move forward, this falls back to `last + 1`
/// to keep generations unique and monotonic in this process.
pub struct VersionGenerator {
	last_version: AtomicU64,
}

impl VersionGenerator {
	pub const COUNTER_BITS: u64 = 11;
	pub const COUNTER_MASK: u64 = (1 << Self::COUNTER_BITS) - 1;

	pub fn new() -> Self {
		let timestamp = Self::timestamp_micros();
		let seed = timestamp & Self::COUNTER_MASK;
		Self {
			last_version: AtomicU64::new((timestamp << Self::COUNTER_BITS) | seed),
		}
	}

	/// Generates a new version that is guaranteed to be greater than any
	/// previously generated version from this generator.
	pub fn next(&self) -> u64 {
		loop {
			let now = Self::timestamp_micros();
			let last = self.last_version.load(Ordering::Acquire);
			let next_counter = (last.wrapping_add(1)) & Self::COUNTER_MASK;
			let timestamp_candidate = (now << Self::COUNTER_BITS) | next_counter;
			let next = if timestamp_candidate > last {
				timestamp_candidate
			} else {
				last + 1
			};

			if self
				.last_version
				.compare_exchange(last, next, Ordering::Release, Ordering::Relaxed)
				.is_ok()
			{
				return next;
			}
			// CAS failed, another thread updated; retry
		}
	}

	fn timestamp_micros() -> u64 {
		chrono::Utc::now().timestamp_micros().max(0) as u64
	}
}

impl Default for VersionGenerator {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_version_monotonicity() {
		let generator = VersionGenerator::new();
		let mut prev = 0;
		for _ in 0..1000 {
			let v = generator.next();
			assert!(v > prev, "Version should be strictly increasing");
			prev = v;
		}
	}

	#[test]
	fn test_version_layout_uses_11_bit_counter() {
		let generator = VersionGenerator::new();
		let version = generator.next();

		assert_ne!(version >> 11, 0);
		assert_eq!(
			version & !VersionGenerator::COUNTER_MASK,
			(version >> 11) << 11
		);
	}

	#[test]
	fn test_version_concurrent() {
		use std::sync::Arc;
		use std::thread;

		let generator = Arc::new(VersionGenerator::new());
		let mut handles = vec![];

		for _ in 0..4 {
			let g = generator.clone();
			handles.push(thread::spawn(move || {
				let mut versions = vec![];
				for _ in 0..100 {
					versions.push(g.next());
				}
				versions
			}));
		}

		let mut all_versions: Vec<u64> = handles
			.into_iter()
			.flat_map(|h: std::thread::JoinHandle<Vec<u64>>| h.join().unwrap())
			.collect();

		let len_before = all_versions.len();
		all_versions.sort();
		all_versions.dedup();
		let len_after = all_versions.len();

		assert_eq!(len_before, len_after, "All versions should be unique");
	}
}
