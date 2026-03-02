//! Configuration system.
//!
//! TOML parsing, resolution, validation, and merge semantics.
//! See §6 of the technical spec.

use serde::Deserialize;

/// Top-level configuration structure deserialized from TOML.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CcvvConfig {
    #[serde(default)]
    pub settings: Settings,
}

/// Feature toggles and limits.
#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    /// Enable unicode normalization (Stage 2). Default: true.
    #[serde(default = "default_true")]
    pub normalize_unicode: bool,

    /// Enable whitespace cleanup (Stage 3). Default: true.
    #[serde(default = "default_true")]
    pub whitespace_cleanup: bool,

    /// Enable agent artifact stripping (Stage 4). Default: true.
    #[serde(default = "default_true")]
    pub agent_strip: bool,

    /// Enable structural detection (Stage 5). Default: true.
    #[serde(default = "default_true")]
    pub structural_detection: bool,

    /// Enable URL cleaning (Stage 6). Default: true.
    #[serde(default = "default_true")]
    pub url_cleaning: bool,

    /// Enable auto-wrapper / backtick wrapping (Stage 7). Default: false.
    #[serde(default)]
    pub auto_wrapper: bool,

    /// Enable user-defined regex rules (Stage 8). Default: true.
    #[serde(default = "default_true")]
    pub user_rules: bool,

    /// Enable sensitive content filter. Default: true.
    #[serde(default = "default_true")]
    pub sensitive_filter: bool,

    /// Maximum input size in bytes. Default: 1048576 (1 MB).
    #[serde(default = "default_max_input_bytes")]
    pub max_input_bytes: usize,

    /// Double-tap detection window in milliseconds. Default: 450.
    #[serde(default = "default_double_tap_window_ms")]
    pub double_tap_window_ms: u32,

    /// Store raw clipboard content in history. Default: false.
    #[serde(default)]
    pub history_store_raw: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            normalize_unicode: true,
            whitespace_cleanup: true,
            agent_strip: true,
            structural_detection: true,
            url_cleaning: true,
            auto_wrapper: false,
            user_rules: true,
            sensitive_filter: true,
            max_input_bytes: 1_048_576,
            double_tap_window_ms: 450,
            history_store_raw: false,
        }
    }
}

/// Resolved configuration with compiled regexes and merged settings.
/// Produced from `CcvvConfig` after validation and profile overlay.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub settings: Settings,
    // TODO: compiled URL deny list, domain overrides, user rules, exclusions
}

impl Default for ResolvedConfig {
    fn default() -> Self {
        ResolvedConfig {
            settings: Settings::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_max_input_bytes() -> usize {
    1_048_576
}

fn default_double_tap_window_ms() -> u32 {
    450
}
