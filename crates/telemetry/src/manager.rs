use std::sync::Arc;
use std::sync::OnceLock;

use crate::TelemetryError;
use crate::logger;
use crate::trace;

/// Unified telemetry entrypoint for log + trace initialization.
pub struct TelemetryManager {
	logger: logger::Logger,
	trace: trace::Tracer,
}

impl TelemetryManager {
	/// Initialize logging and fastrace collector.
	#[fastrace::trace]
	pub fn init(
		level: &str,
		output: logger::LogOutput,
		trace_enabled: bool,
		trace_endpoint: String,
	) -> Result<Self, TelemetryError> {
		let logger = logger::Logger::init(level, output)?;
		let trace = trace::Tracer::init(trace_enabled, trace_endpoint)?;
		Ok(Self { logger, trace })
	}

	pub fn disabled() -> Self {
		Self {
			logger: logger::Logger::disabled(),
			trace: trace::Tracer::disabled(),
		}
	}

	/// Reload the active logger filter.
	pub fn reload_log_level(&self, level: &str) -> Result<(), TelemetryError> {
		self.logger.reload_log_level(level)
	}

	/// Flush pending telemetry records before process shutdown.
	pub fn flush(&self) {
		self.trace.flush();
	}
}

pub struct GlobalTelemetryManager {
	inner: OnceLock<Arc<TelemetryManager>>,
}

impl GlobalTelemetryManager {
	pub const fn new() -> Self {
		Self {
			inner: OnceLock::new(),
		}
	}

	pub fn init(&self, telemetry_manager: Arc<TelemetryManager>) {
		let _ = self.inner.set(telemetry_manager);
	}

	pub fn load(&self) -> &Arc<TelemetryManager> {
		self.inner
			.get()
			.expect("Telemetry manager is not initialized")
	}
}

impl Default for GlobalTelemetryManager {
	fn default() -> Self {
		Self::new()
	}
}

pub static TELEMETRY_MANAGER: GlobalTelemetryManager = GlobalTelemetryManager::new();
