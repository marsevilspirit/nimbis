use crate::TelemetryError;
use crate::logger;
use crate::trace;

/// Unified telemetry entrypoint for log + trace initialization.
pub struct TelemetryManager {
	trace: trace::TraceManager,
}

impl TelemetryManager {
	/// Initialize logging and fastrace collector.
	pub fn init(
		level: &str,
		output: logger::LogOutput,
		trace_enabled: bool,
	) -> Result<Self, TelemetryError> {
		logger::init(level, output)?;
		let trace = trace::TraceManager::init(trace_enabled)?;
		Ok(Self { trace })
	}

	/// Flush pending telemetry records before process shutdown.
	pub fn flush(&self) {
		self.trace.flush();
	}
}
