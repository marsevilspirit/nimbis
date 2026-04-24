use crate::TelemetryError;
use crate::logger;
use crate::trace;

/// Unified telemetry entrypoint for log + trace initialization.
pub struct TelemetryManager;

impl TelemetryManager {
	/// Initialize logging and fastrace collector.
	pub fn init(level: &str, output: logger::LogOutput) -> Result<(), TelemetryError> {
		logger::init(level, output)?;
		trace::init()?;
		Ok(())
	}
}
