pub mod logger;

// Re-export logger initialization for convenience
pub use logger::TelemetryError;
pub use logger::init;
pub use logger::reload_log_level;
