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

/// Default exclusion bundle IDs for password managers.
pub(crate) const DEFAULT_EXCLUSION_BUNDLE_IDS: &[&str] = &[
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.lastpass.LastPass",
    "com.bitwarden.desktop",
    "org.keepassxc.keepassxc",
];

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
    pub table_cell_picker: Option<bool>,
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

/// Double-tap detection window setting.
/// Can be a fixed millisecond value or adaptive ("auto").
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DoubleTapSetting {
    Fixed(u32),
    Adaptive(String), // "auto"
}

impl Default for DoubleTapSetting {
    fn default() -> Self {
        DoubleTapSetting::Fixed(450)
    }
}

/// Em-dash replacement configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct EmDashConfig {
    #[serde(default = "default_em_dash_replace")]
    pub replace: String,
}

impl Default for EmDashConfig {
    fn default() -> Self {
        EmDashConfig {
            replace: "--".to_string(),
        }
    }
}

fn default_em_dash_replace() -> String {
    "--".to_string()
}

fn default_table_cell_picker_min_confidence() -> f32 {
    0.75
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

    /// Enable table cell picker UI on detected tables. Default: true.
    #[serde(default = "default_true")]
    pub table_cell_picker: bool,

    /// Minimum confidence to trigger the table cell picker. Default: 0.75.
    #[serde(default = "default_table_cell_picker_min_confidence")]
    pub table_cell_picker_min_confidence: f32,

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

    /// Double-tap detection window. Default: Fixed(450).
    #[serde(default)]
    pub double_tap_window_ms: DoubleTapSetting,

    /// Store raw clipboard content in history. Default: false.
    #[serde(default)]
    pub history_store_raw: bool,

    /// Em-dash replacement config. Default: replace with "--".
    #[serde(default)]
    pub em_dash: EmDashConfig,

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
            table_cell_picker: true,
            table_cell_picker_min_confidence: 0.75,
            auto_wrapper: false,
            user_rules: true,
            sensitive_filter: true,
            max_input_bytes: 1_048_576,
            double_tap_window_ms: DoubleTapSetting::default(),
            history_store_raw: false,
            em_dash: EmDashConfig::default(),
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
    /// Resolved em-dash replacement string (from EmDashConfig).
    pub em_dash_replacement: String,
    /// Resolved double-tap window in ms, or None for adaptive mode.
    pub resolved_double_tap_ms: Option<u32>,
    /// Per-domain URL parameter overrides.
    pub url_domain_overrides: std::collections::HashMap<String, DomainOverride>,
}

impl Default for ResolvedConfig {
    fn default() -> Self {
        ResolvedConfig {
            settings: Settings::default(),
            compiled_rules: Vec::new(),
            url_global_deny: Vec::new(),
            exclusion_bundle_ids: Vec::new(),
            em_dash_replacement: "--".to_string(),
            resolved_double_tap_ms: Some(450),
            url_domain_overrides: std::collections::HashMap::new(),
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
            let home = std::env::var("HOME")
                .map_err(|_| CcvvError::Config("HOME environment variable not set".to_string()))?;
            std::path::PathBuf::from(home)
                .join(".ccvv")
                .join("config.toml")
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
        if mode != 0o600 && mode != 0o644 {
            return Err(CcvvError::ConfigIntegrity(format!(
                "Config file {:?} has permissions {:o}, expected 0600 or 0644",
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
    macro_rules! apply_overlay {
        ($settings:expr, $overlay:expr, $($field:ident),+ $(,)?) => {
            $(
                if let Some(v) = $overlay.$field {
                    $settings.$field = v;
                }
            )+
        };
    }

    if let Some(profile_name) = profile {
        if let Some(profiles) = &config.profiles {
            if let Some(overlay) = profiles.get(profile_name) {
                apply_overlay!(
                    settings,
                    overlay,
                    normalize_unicode,
                    whitespace_cleanup,
                    agent_strip,
                    structural_detection,
                    url_cleaning,
                    table_cell_picker,
                    auto_wrapper,
                    user_rules,
                    sensitive_filter,
                );
            } else {
                return Err(CcvvError::Config(format!(
                    "Profile '{}' not found in config",
                    profile_name
                )));
            }
        }
    }

    let compiled_rules = compile_user_rules(config)?;
    let (url_global_deny, url_domain_overrides, exclusion_bundle_ids) =
        merge_deny_lists(config);

    // Validate settings ranges
    if settings.max_input_bytes == 0 {
        return Err(CcvvError::Config("max_input_bytes must be > 0".to_string()));
    }
    if !(0.0..=1.0).contains(&settings.table_cell_picker_min_confidence) {
        return Err(CcvvError::Config(
            "table_cell_picker_min_confidence must be between 0.0 and 1.0".to_string(),
        ));
    }

    // Resolve double-tap window
    let resolved_double_tap_ms = match &settings.double_tap_window_ms {
        DoubleTapSetting::Fixed(ms) => {
            if *ms < 100 || *ms > 2000 {
                return Err(CcvvError::Config(
                    "double_tap_window_ms must be between 100 and 2000".to_string(),
                ));
            }
            Some(*ms)
        }
        DoubleTapSetting::Adaptive(_) => None,
    };

    // Resolve em-dash replacement
    let em_dash_replacement = settings.em_dash.replace.clone();

    Ok(ResolvedConfig {
        settings,
        compiled_rules,
        url_global_deny,
        exclusion_bundle_ids,
        em_dash_replacement,
        resolved_double_tap_ms,
        url_domain_overrides,
    })
}

fn compile_user_rules(config: &CcvvConfig) -> Result<Vec<CompiledUserRule>, CcvvError> {
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
                CcvvError::Config(format!("Invalid regex in rule '{}': {}", rule.name, e))
            })?;
            compiled_rules.push(CompiledUserRule {
                name: rule.name.clone(),
                regex,
                replacement: rule.replacement.clone(),
            });
        }
    }
    Ok(compiled_rules)
}

fn merge_deny_lists(
    config: &CcvvConfig,
) -> (
    Vec<String>,
    std::collections::HashMap<String, DomainOverride>,
    Vec<String>,
) {
    let mut url_global_deny: Vec<String> = crate::transforms::url::DEFAULT_DENY_PARAMS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut url_domain_overrides = std::collections::HashMap::new();
    if let Some(url_params) = &config.url_params {
        url_global_deny.extend(url_params.global_deny.iter().cloned());
        url_domain_overrides = url_params.domains.clone();
    }

    let mut exclusion_bundle_ids: Vec<String> = DEFAULT_EXCLUSION_BUNDLE_IDS
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(exclusions) = &config.exclusions {
        exclusion_bundle_ids.extend(exclusions.bundle_ids.iter().cloned());
    }

    (url_global_deny, url_domain_overrides, exclusion_bundle_ids)
}

/// Validate a config without resolving it. Returns validation errors.
pub fn validate_config(config: &CcvvConfig) -> Vec<String> {
    let mut errors = Vec::new();
    errors.extend(validate_user_rules(config));
    errors.extend(validate_settings_ranges(&config.settings));
    errors.extend(validate_profile_names(&config.profiles));
    errors.extend(validate_bundle_ids(&config.exclusions));
    errors
}

fn validate_user_rules(config: &CcvvConfig) -> Vec<String> {
    let mut errors = Vec::new();
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
                errors.push(format!("Invalid regex in rule '{}': {}", rule.name, e));
            }
        }
    }
    errors
}

fn validate_settings_ranges(settings: &Settings) -> Vec<String> {
    let mut errors = Vec::new();
    if settings.max_input_bytes == 0 {
        errors.push("max_input_bytes must be > 0".to_string());
    }
    if !(0.0..=1.0).contains(&settings.table_cell_picker_min_confidence) {
        errors.push("table_cell_picker_min_confidence must be between 0.0 and 1.0".to_string());
    }
    if let DoubleTapSetting::Fixed(ms) = settings.double_tap_window_ms {
        if !(100..=2000).contains(&ms) {
            errors.push("double_tap_window_ms must be between 100 and 2000".to_string());
        }
    }
    errors
}

fn validate_profile_names(
    profiles: &Option<std::collections::HashMap<String, ProfileOverride>>,
) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(profiles) = profiles {
        for name in profiles.keys() {
            if name.is_empty() {
                errors.push("Profile name cannot be empty".to_string());
            }
        }
    }
    errors
}

fn validate_bundle_ids(exclusions: &Option<ExclusionsConfig>) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(exclusions) = exclusions {
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
        assert!(matches!(
            config.settings.double_tap_window_ms,
            DoubleTapSetting::Fixed(300)
        ));
        assert_eq!(config.rules.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_toml_parsing_em_dash() {
        let toml_str = r#"
[settings.em_dash]
replace = "—"
"#;
        let config: CcvvConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.settings.em_dash.replace, "\u{2014}");
    }

    #[test]
    fn test_toml_parsing_double_tap_auto() {
        let toml_str = r#"
[settings]
double_tap_window_ms = "auto"
"#;
        let config: CcvvConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(
            config.settings.double_tap_window_ms,
            DoubleTapSetting::Adaptive(_)
        ));
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
        assert!(result.is_ok()); // default config has no profiles
    }

    #[test]
    fn test_resolve_em_dash_replacement() {
        let config = CcvvConfig::default();
        let resolved = resolve_config(&config, None).unwrap();
        assert_eq!(resolved.em_dash_replacement, "--");
    }

    #[test]
    fn test_resolve_double_tap_adaptive() {
        let toml_str = r#"
[settings]
double_tap_window_ms = "auto"
"#;
        let config: CcvvConfig = toml::from_str(toml_str).unwrap();
        let resolved = resolve_config(&config, None).unwrap();
        assert!(resolved.resolved_double_tap_ms.is_none());
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
        config.settings.double_tap_window_ms = DoubleTapSetting::Fixed(50); // too low
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
        // URL deny: defaults + user additions
        assert!(resolved.url_global_deny.contains(&"utm_source".to_string()));
        assert!(resolved
            .url_global_deny
            .contains(&"custom_tracking".to_string()));
        // Exclusions: defaults + user additions
        assert!(resolved
            .exclusion_bundle_ids
            .contains(&"com.1password.1password".to_string()));
        assert!(resolved
            .exclusion_bundle_ids
            .contains(&"com.example.app".to_string()));
    }

    #[test]
    fn test_default_config_has_exclusions() {
        let config = CcvvConfig::default();
        let resolved = resolve_config(&config, None).unwrap();
        assert_eq!(
            resolved.exclusion_bundle_ids.len(),
            DEFAULT_EXCLUSION_BUNDLE_IDS.len()
        );
        assert!(resolved
            .exclusion_bundle_ids
            .contains(&"com.1password.1password".to_string()));
    }

    #[test]
    fn test_default_config_has_url_deny() {
        let config = CcvvConfig::default();
        let resolved = resolve_config(&config, None).unwrap();
        assert!(resolved.url_global_deny.contains(&"utm_source".to_string()));
        assert!(resolved.url_global_deny.contains(&"fbclid".to_string()));
    }
}
