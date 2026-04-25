use std::borrow::Cow;

use fastrace::collector::Config;
use fastrace_opentelemetry::OpenTelemetryReporter;
use opentelemetry::InstrumentationScope;
use opentelemetry::KeyValue;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;

use crate::TelemetryError;

/// Owns fastrace collection state for the telemetry manager.
pub struct Tracer {
	enabled: bool,
}

impl Tracer {
	pub fn disabled() -> Self {
		Self { enabled: false }
	}

	/// Initializes fastrace collection when enabled.
	///
	/// Callers must validate that an enabled tracer has a non-empty OTLP
	/// endpoint.
	pub fn init(enabled: bool, endpoint: String) -> Result<Self, TelemetryError> {
		if enabled {
			if endpoint.is_empty() {
				return Err(TelemetryError::TraceInitFailed(
					"trace_endpoint must be set when trace collection is enabled".into(),
				));
			}

			let reporter = OpenTelemetryReporter::new(
				SpanExporter::builder()
					.with_tonic()
					.with_endpoint(endpoint.clone())
					.with_protocol(opentelemetry_otlp::Protocol::Grpc)
					.with_timeout(std::time::Duration::from_secs(10)) // Use default 10s if constant is not easily accessible
					.build()
					.expect("initialize otlp exporter"),
				Cow::Owned(
					Resource::builder()
						.with_attributes([KeyValue::new("service.name", "nimbis")])
						.build(),
				),
				InstrumentationScope::builder("nimbis")
					.with_version(env!("CARGO_PKG_VERSION"))
					.build(),
			);
			fastrace::set_reporter(reporter, Config::default());
			log::info!(
				"Tracer initialized successfully with OTLP exporter to {}",
				endpoint
			);
		} else {
			log::info!("Tracer is disabled");
		}

		Ok(Self { enabled })
	}

	/// Flushes pending fastrace records if tracing was initialized.
	pub fn flush(&self) {
		if self.enabled {
			fastrace::flush();
		}
	}
}
