use std::fs::File as StdFile;
use std::fs::OpenOptions;
use std::io::Write;
use std::io::{self};
use std::path::PathBuf;

use chrono::DateTime;
use chrono::Datelike;
use chrono::Local;
use chrono::TimeZone;
use chrono::Timelike;
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

pub struct Logger {
	reload_handle: Option<ReloadHandle>,
	// Keep the background writer alive for file logging.
	_guard: Option<LoggerGuard>,
}

impl Logger {
	fn new(reload_handle: ReloadHandle, guard: LoggerGuard) -> Self {
		Self {
			reload_handle: Some(reload_handle),
			_guard: Some(guard),
		}
	}

	pub fn disabled() -> Self {
		Self {
			reload_handle: None,
			_guard: None,
		}
	}

	/// Initialize the logger with the provided log level.
	pub fn init(level: &str, output: LogOutput) -> Result<Self, TelemetryError> {
		let env_filter = EnvFilter::new(level);

		let is_file = output.is_file();
		let (filter_layer, reload_handle) = reload::Layer::new(env_filter);
		let file_guard = output.init(filter_layer)?;

		log::info!(
			"Logger initialized successfully (level: {}, output: {})",
			level,
			if is_file { "file" } else { "terminal" }
		);

		Ok(Self::new(reload_handle, file_guard))
	}

	/// Reload the log level dynamically.
	pub fn reload_log_level(&self, level: &str) -> Result<(), TelemetryError> {
		validate_log_level(level)?;

		let Some(reload_handle) = self.reload_handle.as_ref() else {
			return Ok(());
		};

		let new_filter = EnvFilter::new(level.to_lowercase());
		reload_handle
			.reload(new_filter)
			.map_err(|e| TelemetryError::ReloadFailed(e.to_string()))
	}
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Terminal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
	path: PathBuf,
	rotation: LogRotation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogOutput {
	Terminal(Terminal),
	File(File),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogRotation {
	Minutely,
	Hourly,
	#[default]
	Daily,
	Never,
}

impl LogRotation {
	pub fn from_mode(mode: &str) -> Result<Self, TelemetryError> {
		match mode.trim().to_ascii_lowercase().as_str() {
			"minutely" => Ok(Self::Minutely),
			"hourly" => Ok(Self::Hourly),
			"daily" => Ok(Self::Daily),
			"never" => Ok(Self::Never),
			_ => Err(TelemetryError::InvalidLogRotation(mode.to_string())),
		}
	}
}

pub struct CustomRollingFile {
	directory: PathBuf,
	file_stem: String,
	extension: Option<String>,
	rotation: LogRotation,
	active_path: PathBuf,
	active_file: Option<StdFile>,
	next_rotation_time: Option<std::time::SystemTime>,
}

impl CustomRollingFile {
	pub fn new(path: impl Into<PathBuf>, rotation: LogRotation) -> Result<Self, TelemetryError> {
		let path = path.into();
		let directory = path
			.parent()
			.unwrap_or(std::path::Path::new(""))
			.to_path_buf();

		if !directory.exists() && directory.to_string_lossy() != "" {
			std::fs::create_dir_all(&directory)
				.map_err(|e| TelemetryError::InitFailed(e.to_string()))?;
		}

		let file_stem = path
			.file_stem()
			.ok_or_else(|| {
				TelemetryError::InitFailed(format!("log file has no stem: {}", path.display()))
			})?
			.to_string_lossy()
			.into_owned();
		let extension = path.extension().map(|e| e.to_string_lossy().into_owned());

		let mut appender = Self {
			directory,
			file_stem,
			extension,
			rotation,
			active_path: path.clone(),
			active_file: None,
			next_rotation_time: None,
		};

		if rotation != LogRotation::Never {
			appender.archive_active_file();
		}

		appender.open_active_file()?;

		Ok(appender)
	}

	fn archive_active_file(&mut self) {
		if self.active_path.exists() {
			let meta = std::fs::metadata(&self.active_path);
			let modified_time: DateTime<Local> = meta
				.and_then(|m| m.modified())
				.unwrap_or_else(|_| std::time::SystemTime::now())
				.into();

			let timestamp = modified_time.format("%Y-%m-%d-%H-%M-%3f").to_string();

			let mut archive_name = format!("{}-{}", self.file_stem, timestamp);
			if let Some(ext) = &self.extension {
				archive_name.push('.');
				archive_name.push_str(ext);
			}
			let archive_path = self.directory.join(archive_name);

			if let Err(e) = std::fs::rename(&self.active_path, &archive_path) {
				panic!("telemetry: failed to rotate log file: {}", e);
			}
		}
	}

	fn calculate_next_rotation(
		now: DateTime<Local>,
		rotation: LogRotation,
	) -> Option<std::time::SystemTime> {
		match rotation {
			LogRotation::Minutely => {
				let next = now + chrono::Duration::minutes(1);
				Local
					.with_ymd_and_hms(
						next.year(),
						next.month(),
						next.day(),
						next.hour(),
						next.minute(),
						0,
					)
					.latest()
					.or(Some(next))
					.map(|dt| dt.into())
			}
			LogRotation::Hourly => {
				let next = now + chrono::Duration::hours(1);
				Local
					.with_ymd_and_hms(next.year(), next.month(), next.day(), next.hour(), 0, 0)
					.latest()
					.or(Some(next))
					.map(|dt| dt.into())
			}
			LogRotation::Daily => {
				let next = now + chrono::Duration::days(1);
				Local
					.with_ymd_and_hms(next.year(), next.month(), next.day(), 0, 0, 0)
					.latest()
					.or(Some(next))
					.map(|dt| dt.into())
			}
			LogRotation::Never => None,
		}
	}

	fn open_active_file(&mut self) -> Result<(), TelemetryError> {
		let file = OpenOptions::new()
			.create(true)
			.append(true)
			.open(&self.active_path)
			.map_err(|e| TelemetryError::InitFailed(e.to_string()))?;

		self.active_file = Some(file);
		self.next_rotation_time = Self::calculate_next_rotation(Local::now(), self.rotation);
		Ok(())
	}
}

impl Write for CustomRollingFile {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		let should_rotate = match self.next_rotation_time {
			Some(time) => std::time::SystemTime::now() >= time,
			None => false,
		};

		if should_rotate && self.rotation != LogRotation::Never {
			self.active_file = None;
			self.archive_active_file();
			if let Err(e) = self.open_active_file() {
				return Err(io::Error::other(e.to_string()));
			}
		}

		if let Some(file) = &mut self.active_file {
			file.write(buf)
		} else {
			Err(io::Error::other("file closed"))
		}
	}

	fn flush(&mut self) -> io::Result<()> {
		if let Some(file) = &mut self.active_file {
			file.flush()
		} else {
			Ok(())
		}
	}
}

impl Terminal {
	fn init(
		self,
		filter_layer: reload::Layer<EnvFilter, Registry>,
	) -> Result<LoggerGuard, TelemetryError> {
		init_subscriber(filter_layer, std::io::stderr, true)?;
		Ok(LoggerGuard::terminal())
	}
}

impl File {
	/// Create a file logger target from a path template.
	///
	/// The parent directory is used as the log directory. The appender always
	/// writes to the active file at the exact path provided. With time-based
	/// rotation (`minutely`, `hourly`, `daily`), the active file is archived
	/// upon rotation. The file stem becomes the prefix, the extension becomes
	/// the suffix, and a rotation timestamp is added to the archived file name.
	/// With `never`, it keeps writing to the single provided path without
	/// archiving.
	pub fn new(path: impl Into<PathBuf>, rotation: LogRotation) -> Self {
		Self {
			path: path.into(),
			rotation,
		}
	}

	fn init(
		self,
		filter_layer: reload::Layer<EnvFilter, Registry>,
	) -> Result<LoggerGuard, TelemetryError> {
		let file_appender = CustomRollingFile::new(self.path.clone(), self.rotation)?;
		let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

		init_subscriber(filter_layer, non_blocking, false)?;
		Ok(LoggerGuard::file(guard))
	}
}

impl LogOutput {
	pub fn from_mode(
		mode: &str,
		log_file_path: impl Into<PathBuf>,
		rotation: LogRotation,
	) -> Result<Self, TelemetryError> {
		match mode.trim().to_ascii_lowercase().as_str() {
			"terminal" => Ok(Self::Terminal(Terminal)),
			"file" => Ok(Self::File(File::new(log_file_path, rotation))),
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
	use_ansi: bool,
) -> Result<(), TelemetryError>
where
	W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
	tracing_subscriber::registry()
		.with(filter_layer)
		.with(
			fmt::layer()
				.with_ansi(use_ansi)
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

pub fn validate_log_level(level: &str) -> Result<(), TelemetryError> {
	const VALID_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];
	let level_lower = level.to_lowercase();

	if !VALID_LEVELS.contains(&level_lower.as_str()) {
		Err(TelemetryError::InvalidLogLevel(level.to_string()))
	} else {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use rstest::rstest;

	use super::*;

	#[rstest]
	#[case("terminal")]
	#[case("TERMINAL")]
	fn test_terminal_log_output(#[case] value: &str) {
		let output =
			LogOutput::from_mode(value, "./nimbis_store/nimbis.log", LogRotation::Daily).unwrap();
		assert!(matches!(output, LogOutput::Terminal(Terminal)));
		assert!(!output.is_file());
	}

	#[rstest]
	#[case("file")]
	#[case("FiLe")]
	fn test_file_log_output(#[case] value: &str) {
		let output =
			LogOutput::from_mode(value, "./nimbis_store/nimbis.log", LogRotation::Daily).unwrap();
		assert!(matches!(output, LogOutput::File(File { .. })));
		assert!(output.is_file());
	}

	#[rstest]
	#[case("stdout")]
	#[case("console")]
	#[case("")]
	fn test_invalid_log_outputs(#[case] value: &str) {
		let result = LogOutput::from_mode(value, "./nimbis_store/nimbis.log", LogRotation::Daily);
		assert!(matches!(result, Err(TelemetryError::InvalidLogOutput(_))));
	}

	#[rstest]
	#[case("minutely", LogRotation::Minutely)]
	#[case("hourly", LogRotation::Hourly)]
	#[case("daily", LogRotation::Daily)]
	#[case("never", LogRotation::Never)]
	#[case("DAILY", LogRotation::Daily)]
	fn test_valid_log_rotations(#[case] value: &str, #[case] expected: LogRotation) {
		assert_eq!(LogRotation::from_mode(value).unwrap(), expected);
	}

	#[rstest]
	#[case("size")]
	#[case("weekly")]
	#[case("")]
	fn test_invalid_log_rotations(#[case] value: &str) {
		let result = LogRotation::from_mode(value);
		assert!(matches!(result, Err(TelemetryError::InvalidLogRotation(_))));
	}

	/// Test that valid log levels pass validation.
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
		assert!(validate_log_level(level).is_ok());
	}

	/// Test that invalid log levels are rejected
	#[rstest]
	#[case("invalid")]
	#[case("foo")]
	#[case("bar")]
	#[case("warning")] // Common mistake (should be "warn")
	#[case("critical")] // Common mistake (not a standard Rust log level)
	fn test_invalid_log_levels(#[case] level: &str) {
		let result = validate_log_level(level);
		assert!(
			matches!(result, Err(TelemetryError::InvalidLogLevel(_))),
			"Expected InvalidLogLevel for: {}",
			level
		);
	}

	// CustomRollingFile tests

	/// Test that creating CustomRollingFile with valid path succeeds
	#[test]
	fn test_custom_rolling_file_new_success() {
		let temp_dir = std::env::temp_dir().join("nimbis_test_new");
		std::fs::create_dir_all(&temp_dir).ok();
		let log_path = temp_dir.join("test.log");

		let result = CustomRollingFile::new(&log_path, LogRotation::Never);
		assert!(result.is_ok());

		// Cleanup
		std::fs::remove_file(&log_path).ok();
		std::fs::remove_dir(&temp_dir).ok();
	}

	/// Test that creating CustomRollingFile with no file stem returns error
	#[test]
	fn test_custom_rolling_file_no_stem_error() {
		// Using a directory path (no file name component) should fail
		let temp_dir = std::env::temp_dir().join("nimbis_test_no_stem");
		std::fs::create_dir_all(&temp_dir).ok();

		// This is essentially a directory, not a file path
		let result = CustomRollingFile::new(&temp_dir, LogRotation::Never);
		assert!(matches!(result, Err(TelemetryError::InitFailed(_))));

		// Cleanup
		std::fs::remove_dir(&temp_dir).ok();
	}

	/// Test that LogRotation::Never skips archiving
	#[test]
	fn test_custom_rolling_file_no_archive_on_never() {
		let temp_dir = std::env::temp_dir().join("nimbis_test_no_archive");
		std::fs::create_dir_all(&temp_dir).ok();
		let log_path = temp_dir.join("test.log");

		// Create initial file
		std::fs::write(&log_path, "initial content").ok();

		let result = CustomRollingFile::new(&log_path, LogRotation::Never);
		assert!(result.is_ok());

		// File should still exist (not archived)
		assert!(log_path.exists());
		let content = std::fs::read_to_string(&log_path).unwrap_or_default();
		assert!(content.contains("initial content"));

		// Cleanup
		std::fs::remove_file(&log_path).ok();
		std::fs::remove_dir(&temp_dir).ok();
	}

	/// Test that archiving happens when rotation is not Never
	#[test]
	fn test_custom_rolling_file_archive_on_rotation() {
		let temp_dir = std::env::temp_dir().join("nimbis_test_archive");
		std::fs::create_dir_all(&temp_dir).ok();
		let log_path = temp_dir.join("test.log");

		// Create initial file with content
		std::fs::write(&log_path, "old content").ok();

		// Create with hourly rotation - should archive existing file
		let result = CustomRollingFile::new(&log_path, LogRotation::Hourly);
		assert!(result.is_ok());

		// Original file should be renamed to archive
		// The active file should exist (empty or new)
		assert!(log_path.exists(), "Active file should exist");

		// There should be an archive file in the directory
		let entries: Vec<_> = std::fs::read_dir(&temp_dir)
			.unwrap()
			.filter_map(|e| e.ok())
			.map(|e| e.file_name().to_string_lossy().to_string())
			.filter(|n| n.starts_with("test-"))
			.collect();
		assert!(
			!entries.is_empty(),
			"Archive file should exist with prefix test-"
		);

		// Cleanup
		for entry in std::fs::read_dir(&temp_dir).unwrap().filter_map(|e| e.ok()) {
			std::fs::remove_file(entry.path()).ok();
		}
		std::fs::remove_dir(&temp_dir).ok();
	}

	/// Test writing to CustomRollingFile
	#[test]
	fn test_custom_rolling_file_write() {
		let temp_dir = std::env::temp_dir().join("nimbis_test_write");
		std::fs::create_dir_all(&temp_dir).ok();
		let log_path = temp_dir.join("test.log");

		let mut file = CustomRollingFile::new(&log_path, LogRotation::Never).unwrap();
		let write_result = file.write_all(b"test message\n");
		assert!(write_result.is_ok());

		let content = std::fs::read_to_string(&log_path).unwrap_or_default();
		assert!(content.contains("test message"));

		// Cleanup
		drop(file);
		std::fs::remove_file(&log_path).ok();
		std::fs::remove_dir(&temp_dir).ok();
	}

	/// Test that creating file in non-existent directory creates it
	#[test]
	fn test_custom_rolling_file_creates_directory() {
		let temp_dir = std::env::temp_dir()
			.join("nimbis_test_nested")
			.join("subdir");
		let log_path = temp_dir.join("test.log");

		// Directory should not exist
		assert!(!temp_dir.exists());

		let result = CustomRollingFile::new(&log_path, LogRotation::Never);
		assert!(result.is_ok());
		assert!(temp_dir.exists());

		// Cleanup
		std::fs::remove_file(&log_path).ok();
		std::fs::remove_dir_all(temp_dir.parent().unwrap()).ok();
	}
}
