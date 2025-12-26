//! # RESP - Redis Serialization Protocol Library
//!
//! A high-performance, zero-copy RESP protocol parser and encoder for Rust.
//!
//! This library provides efficient parsing and encoding of the Redis Serialization Protocol (RESP),
//! supporting both RESP2 and RESP3 specifications.
//!
//! ## Features
//!
//! - **Zero-copy parsing**: Uses `Bytes` for efficient memory management
//! - **RESP2 & RESP3 support**: Full protocol support
//! - **Type-safe API**: Leverages Rust's type system
//! - **High performance**: Optimized for throughput and minimal allocations
//!
//! ## Example
//!
//! ```rust
//! use resp::{RespValue, RespEncoder};
//! use bytes::{Bytes, BytesMut};
//!
//! // Using convenience constructors
//! let cmd = RespValue::array(vec![
//!     RespValue::bulk_string("SET"),
//!     RespValue::bulk_string("key"),
//!     RespValue::bulk_string("value"),
//! ]);
//!
//! // Encode the command
//! let encoded = cmd.encode().unwrap();
//!
//! // Parse a response
//! let mut buf = BytesMut::from(&b"+OK\r\n"[..]);
//! let response = resp::parse(&mut buf).unwrap();
//! assert_eq!(response.as_str(), Some("OK"));
//! ```

mod encoder;
mod error;
mod parser;
mod types;
mod utils;

pub use encoder::RespEncoder;
pub use error::{EncodeError, ParseError, RespError};
pub use parser::parse;
pub use types::RespValue;
