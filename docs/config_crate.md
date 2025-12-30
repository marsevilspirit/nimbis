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

### 2.3 Dynamic Configuration Updates

The `OnlineConfig` macro generates a `set_field` method for the struct:

```rust
pub fn set_field(&mut self, key: &str, value: &str) -> Result<(), String>
```

Example:

```rust
let mut conf = MyConfig::default();

// Successful updates
assert!(conf.set_field("addr", "127.0.0.1").is_ok());
assert!(conf.set_field("port", "8080").is_ok());

// Updating an immutable field will fail
let err = conf.set_field("id", "100");
assert!(err.is_err());
assert_eq!(err.unwrap_err(), "Field 'id' is immutable");

// Updating a non-existent field will fail
assert!(conf.set_field("unknown", "val").is_err());
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

The macro uses the `quote` library to generate the implementation of the `set_field` method:

1.  **Match Statement**: Generates a `match key { ... }` statement.
2.  **Type Conversion**: For mutable fields, uses `std::str::FromStr` to convert the string value to the field's type.
    ```rust
    #field_name_str => {
        match #field_type::from_str(value) {
            Ok(v) => {
                self.#field_name = v;
                Ok(())
            }
            Err(_) => Err(format!("Failed to parse value for field '{}'", ...)),
        }
    }
    ```
3.  **Immutability Protection**: For immutable fields, it directly returns an error.
    ```rust
    #field_name_str => {
        Err(format!("Field '{}' is immutable", #field_name_str))
    }
    ```
4.  **Default Branch**: If no field matches, it returns a "Field not found" error.

In this way, we generate efficient field dispatch logic at compile time, avoiding runtime reflection overhead.
