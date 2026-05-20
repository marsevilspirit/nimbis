# Nimbis Configuration Guide

Nimbis uses a central configuration file. By default, it looks for `config/config.toml`. A full template is provided in `config/config_template.toml`.

Below is a breakdown of all available configurations.

## Server Configuration

Basic server settings determine how Nimbis listens to incoming connections and handles underlying threads.

```toml
# Host and port to bind to
host = "127.0.0.1"
port = 6379

# Number of Tokio runtime worker threads (default: number of CPU cores)
runtime_threads = 8
```

## Object Store Configuration

Nimbis stores data using the `object_store` crate. SlateDB persists data against this object store.

### Local Development

Local development uses a file-backed object store by default.

```toml
object_store_url = "file:nimbis_store"
```

For an in-memory object store (mainly for short-lived tests), no extra options are required:

```toml
object_store_url = "memory:///nimbis/dev"
```

### S3-Compatible Storage (MinIO / AWS)

S3-compatible services such as MinIO or AWS S3 use the `s3://` URL, along with required object store options:

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

The matching setup can also be provided via environment variables:

```bash
NIMBIS_OBJECT_STORE_URL=s3://nimbis/dev
NIMBIS_OBJECT_STORE_OPTION_AWS_REGION=us-east-1
NIMBIS_OBJECT_STORE_OPTION_AWS_ENDPOINT=http://127.0.0.1:9000
NIMBIS_OBJECT_STORE_OPTION_AWS_ACCESS_KEY_ID=minioadmin
NIMBIS_OBJECT_STORE_OPTION_AWS_SECRET_ACCESS_KEY=minioadmin
NIMBIS_OBJECT_STORE_OPTION_AWS_VIRTUAL_HOSTED_STYLE_REQUEST=false
NIMBIS_OBJECT_STORE_OPTION_AWS_ALLOW_HTTP=true
```

## Logging Configuration

Nimbis leverages asynchronous structured logging. You can configure what to emit and where.

```toml
# Log level/filter expression (EnvFilter syntax).
# Example: "nimbis=debug,storage=debug,resp=info"
log_level = "info"

# Log output mode: "terminal" or "file"
# When set to "file", logs are written to nimbis.log in the current working directory.
log_output = "terminal"

# File log rotation: "minutely", "hourly", "daily", or "never"
# Used only when log_output = "file".
log_rotation = "daily"
```

## Tracing (OpenTelemetry) Configuration

Distributed tracing and span collection can be enabled for extensive observability.

```toml
# Enable fastrace collection and OpenTelemetry export
trace_enabled = false

# OpenTelemetry endpoint for exporting traces.
# Required when trace_enabled = true.
trace_endpoint = "http://127.0.0.1:4317"

# Trace sampling ratio between 0.0 and 1.0.
# 1.0 samples everything, 0.0001 (0.01%) is default for production safety.
trace_sampling_ratio = 0.0001

# OTLP transport protocol: "grpc", "http_binary", or "http_json".
trace_protocol = "grpc"

# Export timeout in seconds for each OTLP push.
trace_export_timeout_seconds = 10

# Collector report interval in milliseconds.
trace_report_interval_ms = 1000
```

## Redis Compatibility Options

These fields generally serve as mock configurations responding securely to typical Redis administration commands and tools like `redis-benchmark`, keeping compatibility intact without actually enabling native Redis persistence.

```toml
# Placeholder for Redis compatibility (immutable)
save = ""
appendonly = "no"
```
