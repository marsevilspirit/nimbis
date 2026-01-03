use std::time::Duration;

/// Trait for types that support expiration time management.
/// Provides common functionality for tracking and managing TTL (Time To Live).
pub trait Expirable {
	/// Get the expiration timestamp in milliseconds since Unix epoch.
	/// Returns 0 if no expiration is set.
	fn expire_time(&self) -> u64;

	/// Set the expiration timestamp in milliseconds since Unix epoch.
	fn set_expire_time(&mut self, timestamp: u64);

	/// Check if this item has expired.
	///
	/// Returns `false` if no expiration is set (expire_time == 0).
	/// Otherwise, compares the current time with the expiration time.
	fn is_expired(&self) -> bool {
		if self.expire_time() == 0 {
			return false;
		}
		let now = chrono::Utc::now().timestamp_millis() as u64;
		now >= self.expire_time()
	}

	/// Set the expiration time to a specific Unix timestamp (in milliseconds).
	fn expire_at(&mut self, timestamp: u64) {
		self.set_expire_time(timestamp);
	}

	/// Set the expiration time to current time + duration.
	fn expire_after(&mut self, duration: Duration) {
		let now = chrono::Utc::now().timestamp_millis() as u64;
		self.set_expire_time(now + duration.as_millis() as u64);
	}

	/// Get the remaining time until expiration.
	///
	/// Returns:
	/// - `None` if no expiration is set
	/// - `Some(Duration::ZERO)` if already expired
	/// - `Some(duration)` with the remaining time otherwise
	fn remaining_ttl(&self) -> Option<Duration> {
		if self.expire_time() == 0 {
			return None;
		}
		let now = chrono::Utc::now().timestamp_millis() as u64;
		if now >= self.expire_time() {
			return Some(Duration::ZERO);
		}
		Some(Duration::from_millis(self.expire_time() - now))
	}
}
