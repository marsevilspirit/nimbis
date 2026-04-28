use std::borrow::Cow;

use fastrace::collector::Config;
use fastrace_opentelemetry::OpenTelemetryReporter;
use opentelemetry::InstrumentationScope;
use opentelemetry::KeyValue;
use opentelemetry_otlp::Protocol;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;

use crate::TelemetryError;

/// Owns fastrace collection state for the telemetry manager.
pub struct Tracer {
	enabled: bool,
}

#[derive(Debug, Clone)]
pub struct TracerConfig {
	pub enabled: bool,
	pub endpoint: String,
	pub sampling_ratio: f64,
	pub protocol: String,
	pub export_timeout_seconds: u64,
	pub report_interval_ms: u64,
}

impl Tracer {
	pub fn disabled() -> Self {
		Self { enabled: false }
	}

	/// Initializes fastrace collection when enabled.
	///
	/// Callers must validate that an enabled tracer has a non-empty OTLP
	/// endpoint.
	pub fn init(config: TracerConfig) -> Result<Self, TelemetryError> {
		if config.enabled {
			if config.endpoint.is_empty() {
				return Err(TelemetryError::TraceInitFailed(
					"trace_endpoint must be set when trace collection is enabled".into(),
				));
			}

			let protocol = match config.protocol.trim().to_ascii_lowercase().as_str() {
				"grpc" => Protocol::Grpc,
				"http_binary" => Protocol::HttpBinary,
				"http_json" => Protocol::HttpJson,
				invalid => {
					return Err(TelemetryError::TraceInitFailed(format!(
						"invalid trace protocol: {invalid}. expected grpc/http_binary/http_json"
					)));
				}
			};

			let exporter = match protocol {
				Protocol::Grpc => SpanExporter::builder()
					.with_tonic()
					.with_endpoint(config.endpoint.clone())
					.with_protocol(protocol)
					.with_timeout(std::time::Duration::from_secs(
						config.export_timeout_seconds,
					))
					.build()
					.map_err(|e| TelemetryError::TraceInitFailed(e.to_string()))?,
				Protocol::HttpBinary | Protocol::HttpJson => SpanExporter::builder()
					.with_http()
					.with_endpoint(config.endpoint.clone())
					.with_protocol(protocol)
					.with_timeout(std::time::Duration::from_secs(
						config.export_timeout_seconds,
					))
					.build()
					.map_err(|e| TelemetryError::TraceInitFailed(e.to_string()))?,
			};

			let reporter = OpenTelemetryReporter::new(
				exporter,
				Cow::Owned(
					Resource::builder()
						.with_attributes([KeyValue::new("service.name", "nimbis")])
						.build(),
				),
				InstrumentationScope::builder("nimbis")
					.with_version(env!("CARGO_PKG_VERSION"))
					.build(),
			);
			fastrace::set_reporter(
				reporter,
				Config::default()
					.report_interval(std::time::Duration::from_millis(config.report_interval_ms)),
			);
			log::info!(
				"Tracer initialized successfully with OTLP exporter to {} (protocol={}, sampling_ratio={}, timeout={}s, report_interval={}ms)",
				config.endpoint,
				config.protocol,
				config.sampling_ratio,
				config.export_timeout_seconds,
				config.report_interval_ms
			);
		} else {
			log::info!("Tracer is disabled");
		}

		Ok(Self {
			enabled: config.enabled,
		})
	}

	/// Flushes pending fastrace records if tracing was initialized.
	pub fn flush(&self) {
		if self.enabled {
			fastrace::flush();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_init_rejects_invalid_trace_protocol() {
		let config = TracerConfig {
			enabled: true,
			endpoint: "http://localhost:4317".into(),
			sampling_ratio: 1.0,
			protocol: "invalid".into(),
			export_timeout_seconds: 10,
			report_interval_ms: 1000,
		};

		let result = Tracer::init(config);
		assert!(matches!(result, Err(TelemetryError::TraceInitFailed(_))));
	}
}
