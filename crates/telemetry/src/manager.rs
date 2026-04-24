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
	pub fn init(
		level: &str,
		output: logger::LogOutput,
		trace_enabled: bool,
	) -> Result<Self, TelemetryError> {
		let logger = logger::Logger::init(level, output)?;
		let trace = trace::Tracer::init(trace_enabled)?;
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
