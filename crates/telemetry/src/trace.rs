use std::sync::OnceLock;

use fastrace::collector::Config;
use fastrace::collector::ConsoleReporter;

use crate::TelemetryError;

static TRACE_INITIALIZED: OnceLock<()> = OnceLock::new();

/// Initializes fastrace collector for nimbis.
///
/// This method is idempotent and can be called multiple times safely.
pub fn init() -> Result<(), TelemetryError> {
	if TRACE_INITIALIZED.get().is_some() {
		return Ok(());
	}

	fastrace::set_reporter(ConsoleReporter, Config::default())
		.map_err(|e| TelemetryError::TraceInitFailed(e.to_string()))?;

	let _ = TRACE_INITIALIZED.set(());
	Ok(())
}
