pub mod logger;
use thiserror::Error;

/// Errors that can occur in the telemetry module
#[derive(Debug, Error)]
pub enum TelemetryError {
	/// Invalid log output provided
	#[error("Invalid log output: {0}. Valid values: terminal, file")]
	InvalidLogOutput(String),

	/// Invalid log rotation provided
	#[error("Invalid log rotation: {0}. Valid values: minutely, hourly, daily, never")]
	InvalidLogRotation(String),

	/// The logger has not been initialized yet
	#[error("Logger not initialized")]
	NotInitialized,

	/// Invalid log level provided
	#[error("Invalid log level: {0}. Valid levels: trace, debug, info, warn, error")]
	InvalidLogLevel(String),

	/// Failed to reload the log level
	#[error("Failed to reload log level: {0}")]
	ReloadFailed(String),

	/// Failed to initialize the logger sink
	#[error("Failed to initialize logger: {0}")]
	InitFailed(String),
}
