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

Derive the `OnlineConfig` trait on your configuration struct. By default, all fields are mutable. You can use the `#[online_config(immutable)]` attribute to mark fields as immutable, or use `#[online_config(mutable)]` to explicitly mark them as mutable (although this is the default behavior, it helps with documentation).

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

The core of the `config` crate is a procedural macro `online_config_derive`, located in `crates/config/src/lib.rs`.

### 3.1 AST Parsing

The macro first uses the `syn` library to parse the input Rust Abstract Syntax Tree (AST), extracting the struct's name and field information.

```rust
let input = parse_macro_input!(input as DeriveInput);
// ... Extract Named fields from Data::Struct ...
```

### 3.2 Attribute Processing

For each field, the macro checks if usage of the `#[online_config(immutable)]` attribute exists.

```rust
let mut is_immutable = false;
for attr in &f.attrs {
    if attr.path().is_ident("online_config") {
        // ... Parse nested meta, look for immutable ...
    }
}
```

Note: The macro currently ignores unknown attributes (like `mutable`), which means they are treated as default behavior (mutable). This allows us to explicitly write `mutable` without breaking compilation.

### 3.3 Code Generation

The macro uses the `quote` library to generate the implementation of the methods:

1.  **set_field**: Generates a `match` statement to dispatch to the correct field. Converts string values using `FromStr` for mutable fields, and returns errors for immutable ones.
2.  **get_field**: Generates a `match` statement to return `self.field.to_string()`.
3.  **list_fields**: Returns a static vector of string literals generated from field names.
4.  **match_fields**: Implements efficient string matching logic (using `strip_prefix`/`strip_suffix`) against the static field list to support wildcards.

In this way, we generate efficient field dispatch logic at compile time, avoiding runtime reflection overhead.
