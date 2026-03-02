//! Configuration system.
//!
//! TOML parsing, resolution, validation, and merge semantics.
//! See §6 of the technical spec.

use std::path::Path;

use regex::Regex;
use serde::Deserialize;

use crate::error::CcvvError;
use crate::transforms::userrules::CompiledUserRule;

/// Top-level configuration structure deserialized from TOML.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CcvvConfig {
    #[serde(default)]
    pub settings: Settings,

    /// URL parameter deny list overrides.
    #[serde(default)]
    pub url_params: Option<UrlParamsConfig>,

    /// Application exclusions.
    #[serde(default)]
    pub exclusions: Option<ExclusionsConfig>,

    /// Named profiles for different use cases.
    #[serde(default)]
    pub profiles: Option<std::collections::HashMap<String, ProfileOverride>>,

    /// User-defined regex rules.
    #[serde(default)]
    pub rules: Option<Vec<UserRuleConfig>>,
}

/// URL parameter configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UrlParamsConfig {
    /// Additional global deny parameters.
    #[serde(default)]
    pub global_deny: Vec<String>,

    /// Per-domain overrides.
    #[serde(default)]
    pub domains: std::collections::HashMap<String, DomainOverride>,
}

/// Per-domain URL parameter override.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DomainOverride {
    /// Parameters to always keep for this domain.
    #[serde(default)]
    pub keep: Vec<String>,

    /// Additional parameters to deny for this domain.
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Application exclusion configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExclusionsConfig {
    /// Bundle IDs to exclude from auto-cleaning.
    #[serde(default)]
    pub bundle_ids: Vec<String>,
}

/// Profile override — only specified fields override the base settings.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProfileOverride {
    pub normalize_unicode: Option<bool>,
    pub whitespace_cleanup: Option<bool>,
    pub agent_strip: Option<bool>,
    pub structural_detection: Option<bool>,
    pub url_cleaning: Option<bool>,
    pub auto_wrapper: Option<bool>,
    pub user_rules: Option<bool>,
    pub sensitive_filter: Option<bool>,
}

/// User-defined rule from config.
#[derive(Debug, Clone, Deserialize)]
pub struct UserRuleConfig {
    pub name: String,
    pub pattern: String,
    pub replacement: String,
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

    /// Em-dash replacement. Default: true (replace with --).
    #[serde(default = "default_true")]
    pub em_dash_replace: bool,

    /// Strip URL scheme. Default: false.
    #[serde(default)]
    pub url_strip_scheme: bool,
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
            em_dash_replace: true,
            url_strip_scheme: false,
        }
    }
}

/// Resolved configuration with compiled regexes and merged settings.
/// Produced from `CcvvConfig` after validation and profile overlay.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub settings: Settings,
    pub compiled_rules: Vec<CompiledUserRule>,
    pub url_global_deny: Vec<String>,
    pub exclusion_bundle_ids: Vec<String>,
}

impl Default for ResolvedConfig {
    fn default() -> Self {
        ResolvedConfig {
            settings: Settings::default(),
            compiled_rules: Vec::new(),
            url_global_deny: Vec::new(),
            exclusion_bundle_ids: Vec::new(),
        }
    }
}

/// Load configuration from a TOML file.
///
/// If `path` is `None`, attempts to load from `~/.ccvv/config.toml`.
/// If the file doesn't exist, returns default config.
pub fn load_config(path: Option<&Path>) -> Result<CcvvConfig, CcvvError> {
    let config_path = match path {
        Some(p) => p.to_path_buf(),
        None => {
            let home = std::env::var("HOME").map_err(|_| {
                CcvvError::Config("HOME environment variable not set".to_string())
            })?;
            std::path::PathBuf::from(home).join(".ccvv").join("config.toml")
        }
    };

    if !config_path.exists() {
        return Ok(CcvvConfig::default());
    }

    // File permission check (owner-only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(&config_path)?;
        let mode = metadata.mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(CcvvError::ConfigIntegrity(format!(
                "Config file {:?} has permissions {:o}, expected owner-only (0600 or 0644)",
                config_path, mode
            )));
        }
    }

    let content = std::fs::read_to_string(&config_path)?;
    let config: CcvvConfig =
        toml::from_str(&content).map_err(|e| CcvvError::Toml(e.to_string()))?;

    Ok(config)
}

/// Resolve configuration: apply profile overlay, compile regexes, merge lists.
pub fn resolve_config(
    config: &CcvvConfig,
    profile: Option<&str>,
) -> Result<ResolvedConfig, CcvvError> {
    let mut settings = config.settings.clone();

    // Apply profile overlay if specified
    if let Some(profile_name) = profile {
        if let Some(profiles) = &config.profiles {
            if let Some(overlay) = profiles.get(profile_name) {
                if let Some(v) = overlay.normalize_unicode {
                    settings.normalize_unicode = v;
                }
                if let Some(v) = overlay.whitespace_cleanup {
                    settings.whitespace_cleanup = v;
                }
                if let Some(v) = overlay.agent_strip {
                    settings.agent_strip = v;
                }
                if let Some(v) = overlay.structural_detection {
                    settings.structural_detection = v;
                }
                if let Some(v) = overlay.url_cleaning {
                    settings.url_cleaning = v;
                }
                if let Some(v) = overlay.auto_wrapper {
                    settings.auto_wrapper = v;
                }
                if let Some(v) = overlay.user_rules {
                    settings.user_rules = v;
                }
                if let Some(v) = overlay.sensitive_filter {
                    settings.sensitive_filter = v;
                }
            } else {
                return Err(CcvvError::Config(format!(
                    "Profile '{}' not found in config",
                    profile_name
                )));
            }
        }
    }

    // Compile user rules
    let mut compiled_rules = Vec::new();
    if let Some(rules) = &config.rules {
        if rules.len() > crate::transforms::userrules::MAX_USER_RULES {
            return Err(CcvvError::Config(format!(
                "Too many user rules: {} (max {})",
                rules.len(),
                crate::transforms::userrules::MAX_USER_RULES
            )));
        }
        for rule in rules {
            let regex = Regex::new(&rule.pattern).map_err(|e| {
                CcvvError::Config(format!(
                    "Invalid regex in rule '{}': {}",
                    rule.name, e
                ))
            })?;
            compiled_rules.push(CompiledUserRule {
                name: rule.name.clone(),
                regex,
                replacement: rule.replacement.clone(),
            });
        }
    }

    // Merge URL deny lists
    let mut url_global_deny = Vec::new();
    if let Some(url_params) = &config.url_params {
        url_global_deny = url_params.global_deny.clone();
    }

    // Merge exclusions
    let exclusion_bundle_ids = config
        .exclusions
        .as_ref()
        .map(|e| e.bundle_ids.clone())
        .unwrap_or_default();

    // Validate settings ranges
    if settings.max_input_bytes == 0 {
        return Err(CcvvError::Config(
            "max_input_bytes must be > 0".to_string(),
        ));
    }
    if settings.double_tap_window_ms < 100 || settings.double_tap_window_ms > 2000 {
        return Err(CcvvError::Config(
            "double_tap_window_ms must be between 100 and 2000".to_string(),
        ));
    }

    Ok(ResolvedConfig {
        settings,
        compiled_rules,
        url_global_deny,
        exclusion_bundle_ids,
    })
}

/// Validate a config without resolving it. Returns validation errors.
pub fn validate_config(config: &CcvvConfig) -> Vec<String> {
    let mut errors = Vec::new();

    // Validate regex rules
    if let Some(rules) = &config.rules {
        if rules.len() > crate::transforms::userrules::MAX_USER_RULES {
            errors.push(format!(
                "Too many user rules: {} (max {})",
                rules.len(),
                crate::transforms::userrules::MAX_USER_RULES
            ));
        }
        for rule in rules {
            if let Err(e) = Regex::new(&rule.pattern) {
                errors.push(format!(
                    "Invalid regex in rule '{}': {}",
                    rule.name, e
                ));
            }
        }
    }

    // Validate settings ranges
    if config.settings.max_input_bytes == 0 {
        errors.push("max_input_bytes must be > 0".to_string());
    }
    if config.settings.double_tap_window_ms < 100
        || config.settings.double_tap_window_ms > 2000
    {
        errors.push("double_tap_window_ms must be between 100 and 2000".to_string());
    }

    // Validate profile references exist
    if let Some(profiles) = &config.profiles {
        for name in profiles.keys() {
            if name.is_empty() {
                errors.push("Profile name cannot be empty".to_string());
            }
        }
    }

    // Validate bundle ID format in exclusions
    if let Some(exclusions) = &config.exclusions {
        for bid in &exclusions.bundle_ids {
            if !bid.contains('.') {
                errors.push(format!(
                    "Bundle ID '{}' doesn't look like a valid bundle ID (missing '.')",
                    bid
                ));
            }
        }
    }

    errors
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CcvvConfig::default();
        assert!(config.settings.normalize_unicode);
        assert!(config.settings.whitespace_cleanup);
        assert!(!config.settings.auto_wrapper); // disabled by default
        assert!(config.settings.sensitive_filter);
    }

    #[test]
    fn test_toml_parsing() {
        let toml_str = r#"
[settings]
auto_wrapper = true
double_tap_window_ms = 300

[[rules]]
name = "strip_copyright"
pattern = "\\(c\\)"
replacement = ""
"#;
        let config: CcvvConfig = toml::from_str(toml_str).unwrap();
        assert!(config.settings.auto_wrapper);
        assert_eq!(config.settings.double_tap_window_ms, 300);
        assert_eq!(config.rules.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_resolve_with_profile() {
        let toml_str = r#"
[settings]
auto_wrapper = false

[profiles.markdown]
auto_wrapper = true
"#;
        let config: CcvvConfig = toml::from_str(toml_str).unwrap();
        let resolved = resolve_config(&config, Some("markdown")).unwrap();
        assert!(resolved.settings.auto_wrapper);
    }

    #[test]
    fn test_resolve_invalid_profile() {
        let config = CcvvConfig::default();
        let result = resolve_config(&config, Some("nonexistent"));
        // No profiles section → no error (profile just not found)
        // With profiles section but wrong name → error
        assert!(result.is_ok()); // default config has no profiles
    }

    #[test]
    fn test_validate_invalid_regex() {
        let config = CcvvConfig {
            rules: Some(vec![UserRuleConfig {
                name: "bad".to_string(),
                pattern: "[invalid".to_string(),
                replacement: "".to_string(),
            }]),
            ..Default::default()
        };
        let errors = validate_config(&config);
        assert!(!errors.is_empty());
        assert!(errors[0].contains("Invalid regex"));
    }

    #[test]
    fn test_validate_too_many_rules() {
        let rules: Vec<UserRuleConfig> = (0..60)
            .map(|i| UserRuleConfig {
                name: format!("rule_{}", i),
                pattern: format!("pattern_{}", i),
                replacement: "".to_string(),
            })
            .collect();
        let config = CcvvConfig {
            rules: Some(rules),
            ..Default::default()
        };
        let errors = validate_config(&config);
        assert!(errors.iter().any(|e| e.contains("Too many user rules")));
    }

    #[test]
    fn test_validate_invalid_double_tap_window() {
        let mut config = CcvvConfig::default();
        config.settings.double_tap_window_ms = 50; // too low
        let errors = validate_config(&config);
        assert!(errors.iter().any(|e| e.contains("double_tap_window_ms")));
    }

    #[test]
    fn test_validate_invalid_bundle_id() {
        let config = CcvvConfig {
            exclusions: Some(ExclusionsConfig {
                bundle_ids: vec!["no-dots-here".to_string()],
            }),
            ..Default::default()
        };
        let errors = validate_config(&config);
        assert!(errors.iter().any(|e| e.contains("Bundle ID")));
    }

    #[test]
    fn test_merge_semantics() {
        let toml_str = r#"
[url_params]
global_deny = ["custom_tracking"]

[exclusions]
bundle_ids = ["com.example.app"]
"#;
        let config: CcvvConfig = toml::from_str(toml_str).unwrap();
        let resolved = resolve_config(&config, None).unwrap();
        assert_eq!(resolved.url_global_deny, vec!["custom_tracking"]);
        assert_eq!(resolved.exclusion_bundle_ids, vec!["com.example.app"]);
    }
}
