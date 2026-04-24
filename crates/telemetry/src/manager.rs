use crate::TelemetryError;
use crate::logger;
use crate::trace;

/// Unified telemetry entrypoint for log + trace initialization.
pub struct TelemetryManager;

impl TelemetryManager {
	/// Initialize logging and fastrace collector.
	pub fn init(
		level: &str,
		output: logger::LogOutput,
		trace_enabled: bool,
	) -> Result<(), TelemetryError> {
		logger::init(level, output)?;
		if trace_enabled {
			trace::init()?;
		}
		Ok(())
	}
}
