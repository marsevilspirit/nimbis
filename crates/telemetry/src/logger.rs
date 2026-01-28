use std::sync::OnceLock;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::Registry;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;

use crate::TelemetryError;

/// Custom time formatter that displays time as "YYYY-MM-DD HH:MM:SS.micros"
struct CustomTimeFormat;

impl FormatTime for CustomTimeFormat {
	fn format_time(&self, w: &mut fmt::format::Writer<'_>) -> std::fmt::Result {
		let now = std::time::SystemTime::now();
		let datetime: chrono::DateTime<chrono::Local> = now.into();
		write!(w, "{}", datetime.format("[%Y-%m-%d %H:%M:%S%.6f]"))
	}
}

type ReloadHandle = reload::Handle<EnvFilter, Registry>;

static RELOAD_HANDLE: OnceLock<ReloadHandle> = OnceLock::new();

/// Initialize the logger with the provided log level
///
/// This sets up a console logger with:
/// - The log level from the `level` parameter
/// - Structured output with timestamps in format: YYYY-MM-DD HH:MM:SS.micros
/// - Target/module information
///
/// # Arguments
///
/// * `level` - The log level filter string (e.g., "info", "debug", "warn")
///
/// # Example
///
/// ```no_run
/// let args = Cli::parse();
/// telemetry::logger::init(&args.log_level);
/// log::info!("Server starting");
/// ```
pub fn init(level: &str) {
	// Initialize with provided level
	let env_filter = EnvFilter::new(level);

	let (filter_layer, reload_handle) = reload::Layer::new(env_filter);
	let _ = RELOAD_HANDLE.set(reload_handle);

	// Initialize the subscriber with formatting layer
	tracing_subscriber::registry()
		.with(filter_layer)
		.with(
			fmt::layer()
				.with_timer(CustomTimeFormat)
				.with_target(false)
				.with_thread_ids(true)
				.with_line_number(false)
				.with_file(false),
		)
		.init();
}

/// Reload the log level dynamically
///
/// # Arguments
///
/// * `level` - The new log level to set. Valid values: trace, debug, info,
///   warn, error
///
/// # Example
///
/// ```no_run
/// # use telemetry::logger::reload_log_level;
/// reload_log_level("debug")?;
/// # Ok::<(), telemetry::TelemetryError>(())
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - The logger has not been initialized
/// - The provided log level is invalid
/// - The reload operation fails
pub fn reload_log_level(level: &str) -> Result<(), TelemetryError> {
	// Validate log level
	const VALID_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];
	let level_lower = level.to_lowercase();

	if !VALID_LEVELS.contains(&level_lower.as_str()) {
		return Err(TelemetryError::InvalidLogLevel(level.to_string()));
	}

	// Get the reload handle
	let handle = RELOAD_HANDLE.get().ok_or(TelemetryError::NotInitialized)?;

	// Create new filter and reload
	let new_filter = EnvFilter::new(&level_lower);
	handle
		.reload(new_filter)
		.map_err(|e| TelemetryError::ReloadFailed(e.to_string()))
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	/// Test that valid log levels pass validation but return NotInitialized
	/// error
	///
	/// Note: We cannot test actual reload in unit tests since init() can only
	/// be called once. These tests validate the level validation logic.
	#[rstest]
	#[case("trace")]
	#[case("debug")]
	#[case("info")]
	#[case("warn")]
	#[case("error")]
	#[case("TRACE")] // Test case insensitivity
	#[case("DEBUG")]
	#[case("INFO")]
	#[case("DeBuG")] // Mixed case
	fn test_valid_log_levels(#[case] level: &str) {
		// We expect NotInitialized error since we haven't called init()
		// but we verify that validation passes (no InvalidLogLevel error)
		let result = reload_log_level(level);
		assert!(
			matches!(result, Err(TelemetryError::NotInitialized)),
			"Expected NotInitialized for valid level: {}",
			level
		);
	}

	/// Test that invalid log levels are rejected
	#[rstest]
	#[case("invalid")]
	#[case("foo")]
	#[case("bar")]
	#[case("warning")] // Common mistake (should be "warn")
	#[case("critical")] // Common mistake (not a standard Rust log level)
	fn test_invalid_log_levels(#[case] level: &str) {
		let result = reload_log_level(level);
		assert!(
			matches!(result, Err(TelemetryError::InvalidLogLevel(_))),
			"Expected InvalidLogLevel for: {}",
			level
		);
	}
}
