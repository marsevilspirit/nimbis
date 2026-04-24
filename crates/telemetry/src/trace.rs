use fastrace::collector::Config;
use fastrace::collector::ConsoleReporter;

use crate::TelemetryError;

/// Owns fastrace collection state for the telemetry manager.
pub struct TraceManager {
	enabled: bool,
}

impl TraceManager {
	pub fn disabled() -> Self {
		Self { enabled: false }
	}

	/// Initializes fastrace collector for nimbis when enabled.
	pub fn init(enabled: bool) -> Result<Self, TelemetryError> {
		if enabled {
			fastrace::set_reporter(ConsoleReporter, Config::default());
		}

		Ok(Self { enabled })
	}

	/// Flushes pending fastrace records if tracing was initialized.
	pub fn flush(&self) {
		if self.enabled {
			fastrace::flush();
		}
	}
}
