//! Error types for ccvv-lib.

use thiserror::Error;

/// All errors that ccvv-lib can produce.
#[derive(Error, Debug)]
pub enum CcvvError {
    /// Configuration file parsing or validation error.
    #[error("config error: {0}")]
    Config(String),

    /// Regex compilation error.
    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),

    /// Database operation error.
    #[error("database error: {0}")]
    Database(String),

    /// I/O error (file read/write, permissions).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parsing error.
    #[error("toml error: {0}")]
    Toml(String),

    /// Config file integrity error (permissions, ownership).
    #[error("config integrity error: {0}")]
    ConfigIntegrity(String),

    /// Database corruption detected.
    #[error("database corrupt: {0}")]
    DatabaseCorrupt(String),

    /// Input exceeds maximum allowed size.
    #[error("input too large: {size} bytes (max {max} bytes)")]
    InputTooLarge { size: usize, max: usize },
}
