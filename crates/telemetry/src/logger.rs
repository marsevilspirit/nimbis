use std::path::PathBuf;
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Registry;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::MakeWriter;
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

struct LoggerGuard {
	_worker_guard: Option<WorkerGuard>,
}

impl LoggerGuard {
	fn terminal() -> Self {
		Self {
			_worker_guard: None,
		}
	}

	fn file(guard: WorkerGuard) -> Self {
		Self {
			_worker_guard: Some(guard),
		}
	}
}

struct LoggerState {
	reload_handle: ReloadHandle,
	// Keep the background writer alive for file logging.
	_guard: LoggerGuard,
}

impl LoggerState {
	fn new(reload_handle: ReloadHandle, guard: LoggerGuard) -> Self {
		Self {
			reload_handle,
			_guard: guard,
		}
	}

	fn reload_handle(&self) -> &ReloadHandle {
		&self.reload_handle
	}
}

static LOGGER_STATE: OnceLock<LoggerState> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Terminal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
	path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogOutput {
	Terminal(Terminal),
	File(File),
}

impl Terminal {
	fn init(
		self,
		filter_layer: reload::Layer<EnvFilter, Registry>,
	) -> Result<LoggerGuard, TelemetryError> {
		init_subscriber(filter_layer, std::io::stderr)?;
		Ok(LoggerGuard::terminal())
	}
}

impl File {
	pub fn new(path: impl Into<PathBuf>) -> Self {
		Self { path: path.into() }
	}

	fn init(
		self,
		filter_layer: reload::Layer<EnvFilter, Registry>,
	) -> Result<LoggerGuard, TelemetryError> {
		let parent = self.path.parent().ok_or_else(|| {
			TelemetryError::InitFailed(format!(
				"log file path has no parent directory: {}",
				self.path.display()
			))
		})?;
		let file_name = self.path.file_name().ok_or_else(|| {
			TelemetryError::InitFailed(format!(
				"log file path has no file name: {}",
				self.path.display()
			))
		})?;
		let file_appender = tracing_appender::rolling::never(parent, file_name);
		let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

		init_subscriber(filter_layer, non_blocking)?;
		Ok(LoggerGuard::file(guard))
	}
}

impl LogOutput {
	pub fn from_mode(
		mode: &str,
		log_file_path: impl Into<PathBuf>,
	) -> Result<Self, TelemetryError> {
		match mode.trim().to_ascii_lowercase().as_str() {
			"terminal" => Ok(Self::Terminal(Terminal)),
			"file" => Ok(Self::File(File::new(log_file_path))),
			_ => Err(TelemetryError::InvalidLogOutput(mode.to_string())),
		}
	}

	pub fn is_file(&self) -> bool {
		matches!(self, Self::File(_))
	}

	fn init(
		self,
		filter_layer: reload::Layer<EnvFilter, Registry>,
	) -> Result<LoggerGuard, TelemetryError> {
		match self {
			Self::Terminal(output) => output.init(filter_layer),
			Self::File(output) => output.init(filter_layer),
		}
	}
}

fn init_subscriber<W>(
	filter_layer: reload::Layer<EnvFilter, Registry>,
	writer: W,
) -> Result<(), TelemetryError>
where
	W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
	tracing_subscriber::registry()
		.with(filter_layer)
		.with(
			fmt::layer()
				.with_timer(CustomTimeFormat)
				.with_target(false)
				.with_thread_ids(true)
				.with_line_number(false)
				.with_file(false)
				.with_writer(writer),
		)
		.try_init()
		.map_err(|e| TelemetryError::InitFailed(e.to_string()))
}

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
/// * `output` - The output sink to use
///
/// # Example
///
/// ```no_run
/// // let args = Cli::parse();
/// let output = telemetry::logger::LogOutput::from_mode("terminal", "./nimbis_data/nimbis.log")?;
/// telemetry::logger::init("info", output)?;
/// log::info!("Server starting");
/// # Ok::<(), telemetry::TelemetryError>(())
/// ```
pub fn init(level: &str, output: LogOutput) -> Result<(), TelemetryError> {
	// Initialize with provided level
	let env_filter = EnvFilter::new(level);

	let (filter_layer, reload_handle) = reload::Layer::new(env_filter);
	let file_guard = output.init(filter_layer)?;

	let _ = LOGGER_STATE.set(LoggerState::new(reload_handle, file_guard));
	Ok(())
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

	let state = LOGGER_STATE.get().ok_or(TelemetryError::NotInitialized)?;

	// Create new filter and reload
	let new_filter = EnvFilter::new(&level_lower);
	state
		.reload_handle()
		.reload(new_filter)
		.map_err(|e| TelemetryError::ReloadFailed(e.to_string()))
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("terminal")]
	#[case("TERMINAL")]
	fn test_terminal_log_output(#[case] value: &str) {
		let output = LogOutput::from_mode(value, "./nimbis_data/nimbis.log").unwrap();
		assert!(matches!(output, LogOutput::Terminal(Terminal)));
		assert!(!output.is_file());
	}

	#[rstest]
	#[case("file")]
	#[case("FiLe")]
	fn test_file_log_output(#[case] value: &str) {
		let output = LogOutput::from_mode(value, "./nimbis_data/nimbis.log").unwrap();
		assert!(matches!(output, LogOutput::File(File { .. })));
		assert!(output.is_file());
	}

	#[rstest]
	#[case("stdout")]
	#[case("console")]
	#[case("")]
	fn test_invalid_log_outputs(#[case] value: &str) {
		let result = LogOutput::from_mode(value, "./nimbis_data/nimbis.log");
		assert!(matches!(result, Err(TelemetryError::InvalidLogOutput(_))));
	}

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
