use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

/// Generates monotonically increasing versions based on seconds timestamps.
/// If the current timestamp is not greater than the last generated version,
/// it increments the last version by 1 to guarantee monotonicity.
pub struct VersionGenerator {
	last_version: AtomicU64,
}

impl VersionGenerator {
	pub fn new() -> Self {
		Self {
			last_version: AtomicU64::new(0),
		}
	}

	/// Generates a new version that is guaranteed to be greater than any
	/// previously generated version from this generator.
	pub fn next(&self) -> u64 {
		let now = chrono::Utc::now().timestamp() as u64;

		loop {
			let last = self.last_version.load(Ordering::Acquire);
			let next = if now > last { now } else { last + 1 };

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
