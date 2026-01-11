use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Custom time formatter that displays time as "YYYY-MM-DD HH:MM:SS.micros"
struct CustomTimeFormat;

impl FormatTime for CustomTimeFormat {
	fn format_time(&self, w: &mut fmt::format::Writer<'_>) -> std::fmt::Result {
		let now = std::time::SystemTime::now();
		let datetime: chrono::DateTime<chrono::Local> = now.into();
		write!(w, "{}", datetime.format("[%Y-%m-%d %H:%M:%S%.6f]"))
	}
}

/// Initialize the logger with default configuration
///
/// This sets up a console logger with:
/// - INFO level by default (can be overridden with RUST_LOG env var)
/// - Structured output with timestamps in format: YYYY-MM-DD HH:MM:SS.micros
/// - Target/module information
///
/// # Example
///
/// ```no_run
/// telemetry::init();
/// tracing::info!("Server starting");
/// ```
pub fn init() {
	// Create env filter with INFO as default level
	// Can be overridden by RUST_LOG environment variable
	let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

	// Initialize the subscriber with formatting layer
	tracing_subscriber::registry()
		.with(env_filter)
		.with(
			fmt::layer()
				.with_timer(CustomTimeFormat)
				.with_target(true)
				.with_thread_ids(true)
				.with_line_number(false)
				.with_file(false),
		)
		.init();
}
