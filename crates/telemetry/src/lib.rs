pub mod logger;
use thiserror::Error;

/// Errors that can occur in the telemetry module
#[derive(Debug, Error)]
pub enum TelemetryError {
	/// The logger has not been initialized yet
	#[error("Logger not initialized")]
	NotInitialized,

	/// Invalid log level provided
	#[error("Invalid log level: {0}. Valid levels: trace, debug, info, warn, error")]
	InvalidLogLevel(String),

	/// Failed to reload the log level
	#[error("Failed to reload log level: {0}")]
	ReloadFailed(String),
}
