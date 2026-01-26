# Config Crate Design and Usage Documentation

The `config` crate provides a lightweight online configuration update mechanism for the Nimbis project. It uses Rust's procedural macros to automatically generate methods for dynamically updating fields in configuration structs, supporting runtime configuration modification via string key-value pairs.

## 1. Introduction

During service operation, we often need to dynamically adjust certain parameters (such as timeout duration, cache size, etc.) without restarting the service. The `config` crate simplifies this implementation significantly through the `OnlineConfig` derive macro. By simply adding annotations to the configuration struct, you can obtain type-safe dynamic update capabilities.

## 2. Usage

### 2.1 Add Dependency

Ensure that the `config` crate is included in your `Cargo.toml`:

```toml
[dependencies]
config = { workspace = true }
```

### 2.2 Define Configuration Struct

Derive the `OnlineConfig` trait on your configuration struct. By default, all fields are mutable. You can use the `#[online_config(immutable)]` attribute to mark fields as immutable, or use `#[online_config(callback = "method_name")]` to trigger a callback when the field changes.

```rust
use config::OnlineConfig;

#[derive(Default, OnlineConfig)]
pub struct MyConfig {
    // Mutable by default, can be modified via set_field("addr", "new_addr")
    pub addr: String,

    // Explicitly declared as mutable
    #[online_config(mutable)]
    pub port: u16,

    // Immutable, set_field("id", "...") will return an error
    #[online_config(immutable)]
    pub id: i32,
    
    // Field with callback - triggers on_log_level_change when updated
    #[online_config(callback = "on_log_level_change")]
    pub log_level: String,
}

impl MyConfig {
    // Callback method invoked when log_level is updated
    fn on_log_level_change(&self) -> Result<(), String> {
        // Perform side effects, e.g., reload logging configuration
        println!("Log level changed to: {}", self.log_level);
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
assert!(conf.set_field("addr", "127.0.0.1").is_ok());

// Retrieval
assert_eq!(conf.get_field("addr").unwrap(), "127.0.0.1");

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

Example:

```rust
// List all
let fields = MyConfig::list_fields();
assert!(fields.contains(&"addr"));

// Wildcard matching
let matches = MyConfig::match_fields("*port"); // Suffix match
let matches = MyConfig::match_fields("addr*"); // Prefix match
let matches = MyConfig::match_fields("*");     // Match all
```

## 3. Implementation Principle

The core of the `config` crate's dynamic logic is the `OnlineConfig` derive macro, located in `crates/config-derive/src/lib.rs`.

### 3.1 AST Parsing

The macro first uses the `syn` library to parse the input Rust Abstract Syntax Tree (AST), extracting the struct's name and field information.

```rust
let input = parse_macro_input!(input as DeriveInput);
// ... Extract Named fields from Data::Struct ...
```

### 3.2 Attribute Processing

For each field, the macro checks for `#[online_config(...)]` attributes including `immutable` and `callback`.

```rust
let mut is_immutable = false;
let mut callback = None;
for attr in &f.attrs {
    if attr.path().is_ident("online_config") {
        // Parse nested meta for immutable or callback
        if meta.path.is_ident("callback") {
            // Extract callback method name
            callback = Some(method_name);
        }
    }
}
```

When a `callback` is specified, the generated `set_field` code will invoke the callback method after updating the field value:

```rust
self.field = new_value;
self.callback_method()?;  // Invoke callback
Ok(())
```

This allows side effects like reloading logging configuration when `log_level` changes.

### 3.3 Code Generation

The macro uses the `quote` library to generate the implementation of the methods:

1.  **set_field**: Generates a `match` statement to dispatch to the correct field. Converts string values using `FromStr` for mutable fields, invokes callbacks if specified, and returns errors for immutable ones.
2.  **get_field**: Generates a `match` statement to return `self.field.to_string()`.
3.  **list_fields**: Returns a static vector of string literals generated from field names.
4.  **match_fields**: Implements efficient string matching logic (using `strip_prefix`/`strip_suffix`) against the static field list to support wildcards.

In this way, we generate efficient field dispatch logic at compile time, avoiding runtime reflection overhead.

## 4. Real-World Example: Dynamic Log Level

The `ServerConfig` in `crates/config/src/lib.rs` demonstrates the callback feature and how it's accessed via the macro:

```rust
// crates/config/src/lib.rs
#[derive(Debug, Clone, OnlineConfig)]
pub struct ServerConfig {
    #[online_config(immutable)]
    pub addr: String,
    
    #[online_config(callback = "on_log_level_change")]
    pub log_level: String,
}

impl ServerConfig {
    fn on_log_level_change(&self) -> Result<(), String> {
        // Triggered by CONFIG SET log_level ...
        // side effects...
        Ok(())
    }
}
```

### 4.1 Accessing Configuration

Instead of manually loading the global `SERVER_CONF`, prefer using the `server_config!` macro for brevity:

```rust
// Access a specific field (returns &field)
let level = config::server_config!(log_level);

// Access the full guard for complex operations
let current = config::server_config!(load);
```

This allows the server to dynamically change its log level at runtime via the `CONFIG SET log_level debug` command without restarting.
