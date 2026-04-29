# Config Design and Usage Documentation

The `config` module in the `nimbis` crate provides a lightweight online configuration update mechanism for the Nimbis project. It uses Rust's procedural macros to automatically generate methods for dynamically updating fields in configuration structs, supporting runtime configuration modification via string key-value pairs.

## 1. Introduction

During service operation, we often need to dynamically adjust certain parameters (such as timeout duration, cache size, etc.) without restarting the service. The `config` module simplifies this implementation significantly through the `OnlineConfig` derive macro. By simply adding annotations to the configuration struct, you can obtain type-safe dynamic update capabilities.

## 2. Usage

### 2.1 Dependency

The `config` module is part of the `nimbis` crate. It depends on the `macros` crate for the `OnlineConfig` derive macro.

#### `nimbis/Cargo.toml`
```toml
[dependencies]
nimbis-macros = { workspace = true }
clap = { workspace = true }
# ... other dependencies
```

### 2.2 Define Configuration Struct

Derive the `OnlineConfig` trait on your configuration struct. By default, all fields are mutable. You can use the `#[online_config(immutable)]` attribute to mark fields as immutable, or use `#[online_config(callback = "method_name")]` to trigger a callback when the field changes.

```rust
use crate::config::OnlineConfig;

#[derive(Default, OnlineConfig)]
pub struct MyConfig {
    // Mutable by default, can be modified via set_field("host", "new_host")
    pub host: String,

    // Explicitly declared as mutable
    #[online_config(mutable)]
    pub port: u16,

    // Immutable, set_field("id", "...") will return an error
    #[online_config(immutable)]
    pub id: i32,
    
    // Field with callback - validates the log filter expression when updated
    #[online_config(callback = "on_log_level_change")]
    pub log_level: String,

    // Immutable startup-only output mode
    #[online_config(immutable)]
    pub log_output: String,

    #[online_config(immutable)]
    pub log_rotation: String,

    // Immutable startup-only fastrace collection switch
    #[online_config(immutable)]
    pub trace_enabled: bool,

    // Immutable OpenTelemetry endpoint used when trace collection is enabled
    #[online_config(immutable)]
    pub trace_endpoint: String,

    // Immutable startup-only sampling ratio (0.0 - 1.0)
    #[online_config(immutable)]
    pub trace_sampling_ratio: f64,

    // Immutable startup-only OTLP transport protocol
    #[online_config(immutable)]
    pub trace_protocol: String,

    // Immutable startup-only OTLP exporter timeout (seconds)
    #[online_config(immutable)]
    pub trace_export_timeout_seconds: u64,

    // Immutable startup-only collector flush/report interval (milliseconds)
    #[online_config(immutable)]
    pub trace_report_interval_ms: u64,
}

impl MyConfig {
    // Callback method invoked when log_level is updated
    fn on_log_level_change(&mut self) -> Result<(), String> {
        // Validate or perform side effects before the new config is committed
        println!("Log level/filter changed to: {}", self.log_level);
        Ok(())
    }
}
```

### 2.3 Dynamic Configuration Updates and Retrieval

The `OnlineConfig` macro generates `set_field` and `get_field` methods for the struct:

```rust
pub fn set_field(&mut self, key: &str, value: &str) -> Result<(), String>
pub fn get_field(&self, key: &str) -> Result<String, String>
```

Example:

```rust
let mut conf = MyConfig::default();

// Successful updates
assert!(conf.set_field("host", "127.0.0.1").is_ok());

// Retrieval
assert_eq!(conf.get_field("host").unwrap(), "127.0.0.1");

// Updating an immutable field will fail
let err = conf.set_field("id", "100");
assert!(err.is_err());
assert_eq!(err.unwrap_err(), "Field 'id' is immutable");

// Getting/Setting a non-existent field will fail
assert!(conf.set_field("unknown", "val").is_err());
assert!(conf.get_field("unknown").is_err());
```

### 2.4 Configuration Inspection

The `OnlineConfig` trait also provides methods to inspect available fields, which is useful for implementing wildcard matching or listing configuration.

```rust
// List all available field names
pub fn list_fields() -> Vec<&'static str>

// Get all fields as key-value pairs
pub fn get_all_fields(&self) -> Vec<(String, String)>

// Match fields by wildcard pattern (*, prefix*, *suffix, *middle*)
pub fn match_fields(pattern: &str) -> Vec<&'static str>
```

### 2.5 Global Configuration

Nimbis uses a global singleton for configuration access:

```rust
use crate::config::SERVER_CONF;

// Access configuration
let config = SERVER_CONF.load();
println!("Host: {}, Port: {}", config.host, config.port);
```

## 3. Implementation Principle

The core of the `config` module's dynamic logic is the `OnlineConfig` derive macro, located in `nimbis-macros/src/lib.rs`.

### 3.1 Code Generation

The macro uses the `quote` library to generate the implementation of the methods:

1.  **set_field**: Generates a `match` statement to dispatch to the correct field. Converts string values using `FromStr` for mutable fields, invokes callbacks if specified, and returns errors for immutable ones.
2.  **get_field**: Generates a `match` statement to return `self.field.to_string()`.
3.  **list_fields**: Returns a static vector of string literals generated from field names.
4.  **match_fields**: Implements efficient string matching logic (using `strip_prefix`/`strip_suffix`) against the static field list to support wildcards.

## 4. Real-World Example: Dynamic Log Level

The `ServerConfig` in `nimbis/src/config.rs` demonstrates the callback feature and how it's accessed via the macro:

```rust
// nimbis/src/config.rs
#[derive(Debug, Clone, OnlineConfig)]
pub struct ServerConfig {
    #[online_config(immutable)]
    pub host: String,

    #[online_config(immutable)]
    pub port: u16,

    #[online_config(callback = "on_log_level_change")]
    pub log_level: String,

    #[online_config(immutable)]
    pub log_output: String,

    #[online_config(immutable)]
    pub log_rotation: String,

    #[online_config(immutable)]
    pub trace_enabled: bool,

    #[online_config(immutable)]
    pub trace_endpoint: String,

    #[online_config(immutable)]
    pub trace_sampling_ratio: f64,

    #[online_config(immutable)]
    pub trace_protocol: String,

    #[online_config(immutable)]
    pub trace_export_timeout_seconds: u64,

    #[online_config(immutable)]
    pub trace_report_interval_ms: u64,

    #[online_config(immutable)]
    pub worker_threads: usize,
}
```

### 4.1 Accessing Configuration

Instead of manually loading the global `SERVER_CONF`, prefer using the `server_config!` macro for brevity:

```rust
// Access a specific field (returns the field value)
let level = server_config!(log_level);

// Access the full configuration Guard for complex operations
let current = SERVER_CONF.load();
```

`log_level` accepts `tracing_subscriber::EnvFilter` syntax, so both a plain level (`info`) and a target-specific filter (`nimbis=debug,storage=debug,resp=info,slatedb=warn,tokio=warn,info`) are valid.

This allows the server to dynamically change its log filter at runtime via commands such as `CONFIG SET log_level nimbis=debug,info` without restarting.

### 4.2 Startup-only Log Output

Nimbis also exposes an immutable `log_output` field for selecting the startup log sink:

- `terminal`: keep writing logs to stderr via the tracing formatter.
- `file`: write logs to `nimbis.log` in the current working directory.

Because log sink selection changes bootstrap behavior, `log_output` is immutable and must be set in the configuration file before startup. Runtime commands such as `CONFIG SET log_output file` will be rejected.

When `log_output = "file"`, the immutable `log_rotation` field controls time-based rotation:

- `minutely`: rotate once per minute.
- `hourly`: rotate once per hour.
- `daily`: rotate once per day. This is the default to avoid unbounded log growth.
- `never`: disable rotation and keep writing to the single file `nimbis.log`.

For the rotating modes, Nimbis uses a custom rolling file implementation that manages log files directly. Logs use `nimbis` as the filename prefix and `.log` as the suffix. When rotation is enabled, the appender adds timestamp-based suffixes to archived log files according to the selected rotation policy.

### 4.3 Startup-only Object Store

Nimbis stores SlateDB data through the `object_store` crate. The immutable `object_store_url` field selects the object store root:

```toml
object_store_url = "file:nimbis_store"
```

Absolute local paths and S3-compatible stores use the same field:

```toml
object_store_url = "file:///tmp/nimbis_store"
object_store_url = "memory:///nimbis/dev"
object_store_url = "s3://nimbis/dev"
```

Cloud-specific options are supplied through `object_store_options`:

```toml
[object_store_options]
aws_region = "us-east-1"
aws_endpoint = "http://127.0.0.1:9000"
aws_access_key_id = "minioadmin"
aws_secret_access_key = "minioadmin"
aws_virtual_hosted_style_request = "false"
aws_allow_http = "true"
```

For MinIO specifically, use an `s3://` URL with a bucket/prefix plus endpoint and credentials:

```toml
object_store_url = "s3://nimbis/dev"

[object_store_options]
aws_region = "us-east-1"
aws_endpoint = "http://127.0.0.1:9000"
aws_access_key_id = "minioadmin"
aws_secret_access_key = "minioadmin"
aws_virtual_hosted_style_request = "false"
aws_allow_http = "true"
```

Equivalent environment variables:

```bash
NIMBIS_OBJECT_STORE_URL=s3://nimbis/dev
NIMBIS_OBJECT_STORE_OPTION_AWS_REGION=us-east-1
NIMBIS_OBJECT_STORE_OPTION_AWS_ENDPOINT=http://127.0.0.1:9000
NIMBIS_OBJECT_STORE_OPTION_AWS_ACCESS_KEY_ID=minioadmin
NIMBIS_OBJECT_STORE_OPTION_AWS_SECRET_ACCESS_KEY=minioadmin
NIMBIS_OBJECT_STORE_OPTION_AWS_VIRTUAL_HOSTED_STYLE_REQUEST=false
NIMBIS_OBJECT_STORE_OPTION_AWS_ALLOW_HTTP=true
```

Environment variables can override these startup values with `NIMBIS_OBJECT_STORE_URL` and `NIMBIS_OBJECT_STORE_OPTION_<KEY>`.

Runtime commands such as `CONFIG SET log_rotation hourly` are rejected for the same reason: rotation is part of bootstrap-only logger setup.

### 4.4 Startup-only Trace Collection

The immutable `trace_enabled` field controls whether Nimbis initializes the fastrace collector during startup:

- `false`: do not start trace collection. This is the default.
- `true`: start fastrace collection.

When `trace_enabled = true`, the immutable `trace_endpoint` field is required and must be a valid `http` or `https` URL with a host, such as `http://localhost:4317`. Traces are exported to that OpenTelemetry endpoint via gRPC.

Additional startup-only trace controls:

- `trace_sampling_ratio`: decimal value in `[0.0, 1.0]`. This controls command-span sampling rate.
- `trace_protocol`: one of `grpc`, `http_binary`, `http_json`.
- `trace_export_timeout_seconds`: timeout for each OTLP export request.
- `trace_report_interval_ms`: maximum interval between collector report cycles; reducing this can reduce burst size.

When `trace_enabled = false`, `trace_endpoint` may be left empty. This is the default configuration and disables trace export entirely.

Environment variables can override file-based trace configuration at startup:

- `NIMBIS_TRACE_ENABLED`
- `NIMBIS_TRACE_ENDPOINT`
- `NIMBIS_TRACE_SAMPLING_RATIO`
- `NIMBIS_TRACE_PROTOCOL`
- `NIMBIS_TRACE_EXPORT_TIMEOUT_SECONDS`
- `NIMBIS_TRACE_REPORT_INTERVAL_MS`

Recommended profiles:

- Local development: `trace_enabled=true`, `trace_sampling_ratio=1.0`, `trace_report_interval_ms=500`, pointing to a local collector.
- Production baseline: `trace_enabled=true`, `trace_sampling_ratio=0.0001` (0.01%), `trace_protocol=grpc`, `trace_export_timeout_seconds=10`, `trace_report_interval_ms=1000`.
- Emergency high-load safety: temporarily set `trace_enabled=false` or reduce `trace_sampling_ratio` toward `0.0`.

Runtime commands such as `CONFIG SET trace_enabled true` or `CONFIG SET trace_endpoint http://localhost:4317` are rejected because fastrace collector setup is part of bootstrap-only telemetry initialization.

## 5. Build-time Configuration

In addition to runtime configuration, Nimbis uses a `build.rs` script in the `nimbis` crate to capture environment information at compile time. These values are embedded into the binary and cannot be changed without recompilation:

- **Git Info**: `NIMBIS_GIT_HASH`, `NIMBIS_GIT_BRANCH`, `NIMBIS_GIT_DIRTY`
- **Build Info**: `NIMBIS_BUILD_DATE`, `NIMBIS_RUSTC_VERSION`, `NIMBIS_TARGET`

These are used primarily by the `logo` module to display detailed version information on startup.
