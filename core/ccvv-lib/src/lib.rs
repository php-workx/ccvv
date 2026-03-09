//! ccvv-lib: Text transformation engine for the ccvv clipboard sanitizer.
//!
//! This library provides the core transform pipeline, configuration system,
//! content classification, history database, and sensitive content detection.
//! It is used by both the macOS Swift app (via C FFI) and the ccvv CLI
//! (via native Rust API).

pub mod classify;
pub mod config;
pub mod error;
pub mod ffi;
pub mod history;
pub mod pipeline;
pub mod secrets;
pub mod table_extract;
pub mod timing;
pub mod transforms;

// Re-export key types for convenience.
pub use config::{CcvvConfig, ResolvedConfig, Settings};
pub use error::CcvvError;
pub use pipeline::Pipeline;
pub use secrets::SecretFilter;
pub use timing::AdaptiveTimingWindow;
pub use transforms::{ContentType, RuleFired, Transform, TransformContext};
