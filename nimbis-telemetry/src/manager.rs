use std::sync::Arc;
use std::sync::OnceLock;

use crate::TelemetryError;
use crate::logger;
use crate::tracer;

/// Unified telemetry entrypoint for log + trace initialization.
pub struct TelemetryManager {
	logger: logger::Logger,
	tracer: tracer::Tracer,
}

impl TelemetryManager {
	/// Initialize logging and fastrace collector.
	#[fastrace::trace]
	pub fn init(
		level: &str,
		output: logger::LogOutput,
		trace_config: tracer::TracerConfig,
	) -> Result<Self, TelemetryError> {
		let logger = logger::Logger::init(level, output)?;
		let tracer = tracer::Tracer::init(trace_config)?;
		Ok(Self { logger, tracer })
	}

	pub fn disabled() -> Self {
		Self {
			logger: logger::Logger::disabled(),
			tracer: tracer::Tracer::disabled(),
		}
	}

	/// Reload the active logger filter.
	pub fn reload_log_level(&self, level: &str) -> Result<(), TelemetryError> {
		self.logger.reload_log_level(level)
	}

	/// Flush pending telemetry records before process shutdown.
	pub fn flush(&self) {
		self.tracer.flush();
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
