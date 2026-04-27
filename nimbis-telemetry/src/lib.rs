pub mod logger;
pub mod manager;
pub mod trace;
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

	/// The logger was initialized more than once
	#[error("Logger already initialized")]
	AlreadyInitialized,

	/// Invalid log filter expression provided
	#[error("Invalid log level/filter expression: {0}")]
	InvalidLogLevel(String),

	/// Failed to reload the log level
	#[error("Failed to reload log level: {0}")]
	ReloadFailed(String),

	/// Failed to initialize the logger sink
	#[error("Failed to initialize logger: {0}")]
	InitFailed(String),

	/// Failed to initialize fastrace collector
	#[error("Failed to initialize trace collector: {0}")]
	TraceInitFailed(String),
}
