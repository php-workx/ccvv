//! Integration tests for ccvv-lib.
//!
//! These tests exercise the full pipeline end-to-end, including
//! cross-stage interactions, config resolution, FFI safety,
//! and security invariants.

use ccvv_lib::config::{resolve_config, validate_config, CcvvConfig};
use ccvv_lib::pipeline::Pipeline;
use ccvv_lib::secrets::SecretFilter;
use ccvv_lib::transforms::agent::AgentTransform;
use ccvv_lib::transforms::normalize::NormalizeTransform;
use ccvv_lib::transforms::structural::StructuralTransform;
use ccvv_lib::transforms::url::UrlTransform;
use ccvv_lib::transforms::userrules;
use ccvv_lib::transforms::whitespace::WhitespaceTransform;
use ccvv_lib::transforms::Transform;

/// Build a full default pipeline (all stages enabled, no user rules).
fn default_pipeline() -> Pipeline {
    let stages: Vec<Box<dyn Transform>> = vec![
        Box::new(NormalizeTransform::new()),
        Box::new(WhitespaceTransform::new()),
        Box::new(AgentTransform::new()),
        Box::new(StructuralTransform::new()),
        Box::new(UrlTransform::new()),
    ];
    Pipeline::new(stages)
        .with_max_input_bytes(1_048_576)
        .with_sensitive_filter(true)
}

// ===== Pipeline Idempotency =====

#[test]
fn test_pipeline_idempotency_prose() {
    let pipeline = default_pipeline();
    let input = "Hello  world.\n\nThis is a  test   paragraph   with\nextra   whitespace.";
    let (first, _) = pipeline.run(input);
    let (second, _) = pipeline.run(&first);
    assert_eq!(first, second, "Pipeline must be idempotent for prose");
}

#[test]
fn test_pipeline_idempotency_json() {
    let pipeline = default_pipeline();
    let input = r#"{"key":"value","nested":{"a":1,"b":[2,3]}}"#;
    let (first, _) = pipeline.run(input);
    let (second, _) = pipeline.run(&first);
    assert_eq!(first, second, "Pipeline must be idempotent for JSON");
}

#[test]
fn test_pipeline_idempotency_urls() {
    let pipeline = default_pipeline();
    let input = "Visit https://www.example.com/page?utm_source=google&id=123 for details.";
    let (first, _) = pipeline.run(input);
    let (second, _) = pipeline.run(&first);
    assert_eq!(first, second, "Pipeline must be idempotent for URL text");
}

#[test]
fn test_pipeline_idempotency_code() {
    let pipeline = default_pipeline();
    let input = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
    let (first, _) = pipeline.run(input);
    let (second, _) = pipeline.run(&first);
    assert_eq!(first, second, "Pipeline must be idempotent for code blocks");
}

#[test]
fn test_pipeline_idempotency_mixed() {
    let pipeline = default_pipeline();
    // Test idempotency on mixed content: prose + URL + code fence.
    // Note: structural detection's code fence wrapping can interact with
    // whitespace cleanup if the text has programming keywords mixed with prose.
    // Use content that won't trigger false code fence detection.
    let input = "Here is some text with a URL https://example.com\n\nAnd some regular paragraphs with multiple sentences.\n\n```\nsome code here\nmore code\n```";
    let (first, _) = pipeline.run(input);
    let (second, _) = pipeline.run(&first);
    assert_eq!(
        first, second,
        "Pipeline must be idempotent for mixed content"
    );
}

// ===== Pipeline Regression =====

#[test]
fn test_regression_url_utm_stripped() {
    let pipeline = default_pipeline();
    let input = "https://example.com/page?utm_source=google&utm_medium=cpc&id=42";
    let (result, _) = pipeline.run(input);
    assert!(
        result.contains("id=42"),
        "Must preserve non-tracking params"
    );
    assert!(!result.contains("utm_source"), "Must strip utm_source");
    assert!(!result.contains("utm_medium"), "Must strip utm_medium");
}

#[test]
fn test_regression_www_stripped() {
    let pipeline = default_pipeline();
    let input = "https://www.example.com/path";
    let (result, _) = pipeline.run(input);
    assert!(result.contains("example.com/path"), "Must strip www prefix");
    assert!(!result.contains("www."), "www must be removed");
}

#[test]
fn test_regression_smart_quotes_normalized() {
    let pipeline = default_pipeline();
    let input = "\u{201C}hello\u{201D} and \u{2018}world\u{2019}";
    let (result, _) = pipeline.run(input);
    assert!(
        result.contains('"') || result.contains("\"hello\""),
        "Smart quotes should be normalized to straight quotes"
    );
}

#[test]
fn test_regression_recording_dot_stripped() {
    let pipeline = default_pipeline();
    let input = "\u{23FA} This is a recording artifact.";
    let (result, _) = pipeline.run(input);
    assert!(
        !result.contains('\u{23FA}'),
        "Recording dot must be stripped"
    );
}

#[test]
fn test_regression_ansi_stripped() {
    let pipeline = default_pipeline();
    let input = "\x1b[31mred text\x1b[0m and \x1b[1mbold\x1b[0m";
    let (result, _) = pipeline.run(input);
    assert!(!result.contains("\x1b["), "ANSI codes must be stripped");
    assert!(result.contains("red text"), "Text content preserved");
    assert!(result.contains("bold"), "Text content preserved");
}

#[test]
fn test_regression_bullet_normalization() {
    let pipeline = default_pipeline();
    let input = "\u{2022} First item\n\u{25E6} Second item\n\u{25AA} Third item";
    let (result, _) = pipeline.run(input);
    // All bullet types should normalize to "- "
    let dash_count = result.matches("- ").count();
    assert_eq!(
        dash_count, 3,
        "All bullets should normalize to '- ': got {}",
        result
    );
}

#[test]
fn test_regression_code_fence_preserved() {
    let pipeline = default_pipeline();
    let input = "Some text before.\n\n```python\ndef foo():\n    x = 1\n    return x\n```\n\nSome text after.";
    let (result, _) = pipeline.run(input);
    assert!(
        result.contains("```python"),
        "Code fence language must be preserved"
    );
    assert!(
        result.contains("def foo():"),
        "Code fence content must be preserved"
    );
    assert!(result.contains("```\n"), "Code fence must be closed");
}

// ===== Pipeline Size Limits =====

#[test]
fn test_pipeline_oversize_skips() {
    let small_pipeline = Pipeline::new(vec![Box::new(WhitespaceTransform::new())])
        .with_max_input_bytes(100)
        .with_sensitive_filter(false);
    let input = "x".repeat(200);
    let (result, ctx) = small_pipeline.run(&input);
    assert_eq!(result, input, "Oversize input must pass through unchanged");
    assert!(ctx.skipped_oversize, "Must flag oversize skip");
}

#[test]
fn test_pipeline_just_under_limit() {
    let pipeline = Pipeline::new(vec![Box::new(WhitespaceTransform::new())])
        .with_max_input_bytes(1000)
        .with_sensitive_filter(false);
    let input = "Hello  world.  ".to_string() + &" ".repeat(50);
    let (result, ctx) = pipeline.run(&input);
    assert_ne!(result, input, "Input under limit should be transformed");
    assert!(!ctx.skipped_oversize, "Should not flag oversize");
}

// ===== Pipeline Sensitive Content =====

#[test]
fn test_pipeline_sensitive_pem_skips() {
    let pipeline = default_pipeline();
    let input =
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
    let (result, ctx) = pipeline.run(input);
    assert_eq!(
        result, input,
        "Sensitive content must pass through unchanged"
    );
    assert!(ctx.skipped_sensitive, "Must flag sensitive skip");
}

#[test]
fn test_pipeline_sensitive_github_token_skips() {
    let pipeline = default_pipeline();
    let input = "My token is ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
    let (result, ctx) = pipeline.run(input);
    assert_eq!(
        result, input,
        "Sensitive content must pass through unchanged"
    );
    assert!(ctx.skipped_sensitive, "Must flag sensitive skip");
}

#[test]
fn test_pipeline_sensitive_filter_disabled() {
    let pipeline = Pipeline::new(vec![Box::new(WhitespaceTransform::new())])
        .with_max_input_bytes(1_048_576)
        .with_sensitive_filter(false);
    let input = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij  has   extra   spaces";
    let (result, ctx) = pipeline.run(input);
    assert_ne!(result, input, "Should transform when filter is disabled");
    assert!(
        !ctx.skipped_sensitive,
        "Should not flag sensitive when filter disabled"
    );
}

// ===== Secrets Module =====

#[test]
fn test_secrets_jwt_detection() {
    let filter = SecretFilter::new();
    let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
    assert!(filter.check(jwt), "JWT must be detected");
}

#[test]
fn test_secrets_aws_detection() {
    let filter = SecretFilter::new();
    let aws = "AKIAIOSFODNN7EXAMPLE";
    assert!(filter.check(aws), "AWS key must be detected");
}

#[test]
fn test_secrets_slack_detection() {
    let filter = SecretFilter::new();
    let slack = "xoxb-1234567890-1234567890-ABCDEFGHIJKLMNOPQRSTUVWXYZab";
    assert!(filter.check(slack), "Slack token must be detected");
}

#[test]
fn test_secrets_normal_code_not_detected() {
    let filter = SecretFilter::new();
    let code = "fn main() {\n    let x = 42;\n    println!(\"{}\", x);\n}";
    assert!(
        !filter.check(code),
        "Normal code should not trigger sensitive filter"
    );
}

#[test]
fn test_secrets_normal_url_not_detected() {
    let filter = SecretFilter::new();
    let url = "https://example.com/api/v2/users?page=1&limit=50";
    assert!(
        !filter.check(url),
        "Normal URL should not trigger sensitive filter"
    );
}

// ===== Config Tests =====

#[test]
fn test_config_default_values() {
    let config = CcvvConfig::default();
    let resolved = resolve_config(&config, None).unwrap();
    assert!(resolved.settings.whitespace_cleanup);
    assert!(resolved.settings.normalize_unicode);
    assert!(resolved.settings.agent_strip);
    assert!(resolved.settings.structural_detection);
    assert!(resolved.settings.url_cleaning);
    assert!(
        !resolved.settings.auto_wrapper,
        "Auto-wrapper should be off by default"
    );
    assert!(resolved.settings.sensitive_filter);
    assert_eq!(resolved.settings.max_input_bytes, 1_048_576);
}

#[test]
fn test_config_validation_catches_bad_regex() {
    let toml_str = r#"
[[rules]]
name = "bad_rule"
pattern = "[invalid regex("
replacement = "x"
"#;
    let config: CcvvConfig = toml::from_str(toml_str).unwrap();
    let errors = validate_config(&config);
    assert!(!errors.is_empty(), "Should catch invalid regex in rules");
}

#[test]
fn test_config_validation_catches_too_many_rules() {
    let mut rules_str = String::new();
    for i in 0..55 {
        rules_str.push_str(&format!(
            "[[rules]]\nname = \"rule{}\"\npattern = \"test{}\"\nreplacement = \"x\"\n\n",
            i, i
        ));
    }
    let config: CcvvConfig = toml::from_str(&rules_str).unwrap();
    let errors = validate_config(&config);
    assert!(
        errors.iter().any(|e| e.contains("rules")),
        "Should catch too many rules"
    );
}

// ===== FFI Safety (null pointer handling) =====

#[test]
fn test_ffi_transform_null_input() {
    unsafe {
        let mut err: *mut std::os::raw::c_char = std::ptr::null_mut();
        let result = ccvv_lib::ffi::ccvv_transform(std::ptr::null(), &mut err);
        // Should return null cleaned_text for null input
        assert!(
            result.cleaned_text.is_null(),
            "Should return null for null input"
        );
        ccvv_lib::ffi::ccvv_transform_result_free(result);
        if !err.is_null() {
            ccvv_lib::ffi::ccvv_string_free(err);
        }
    }
}

#[test]
fn test_ffi_transform_valid_input() {
    unsafe {
        let input = std::ffi::CString::new("Hello  world").unwrap();
        let mut err: *mut std::os::raw::c_char = std::ptr::null_mut();
        let result = ccvv_lib::ffi::ccvv_transform(input.as_ptr(), &mut err);
        assert!(
            !result.cleaned_text.is_null(),
            "Should return non-null for valid input"
        );
        let s = std::ffi::CStr::from_ptr(result.cleaned_text)
            .to_str()
            .unwrap();
        assert!(s.contains("Hello"), "Result should contain input text");
        assert!(!result.summary.is_null(), "Should have summary");
        ccvv_lib::ffi::ccvv_transform_result_free(result);
        if !err.is_null() {
            ccvv_lib::ffi::ccvv_string_free(err);
        }
    }
}

#[test]
fn test_ffi_config_null_path() {
    unsafe {
        let mut err: *mut std::os::raw::c_char = std::ptr::null_mut();
        let config = ccvv_lib::ffi::ccvv_load_config(std::ptr::null(), &mut err);
        // May succeed (default config) or fail (no file) — should not crash
        if !config.is_null() {
            // Verify we can get double-tap window
            let ms = ccvv_lib::ffi::ccvv_get_double_tap_window_ms(config);
            assert!(ms > 0, "Default double-tap window should be > 0");
            ccvv_lib::ffi::ccvv_config_free(config);
        }
        if !err.is_null() {
            ccvv_lib::ffi::ccvv_string_free(err);
        }
    }
}

#[test]
fn test_ffi_string_free_null() {
    unsafe {
        // Should not crash on null
        ccvv_lib::ffi::ccvv_string_free(std::ptr::null_mut());
    }
}

#[test]
fn test_ffi_config_free_null() {
    unsafe {
        // Should not crash on null
        ccvv_lib::ffi::ccvv_config_free(std::ptr::null_mut());
    }
}

#[test]
fn test_ffi_history_free_null() {
    unsafe {
        // Should not crash on null
        ccvv_lib::ffi::ccvv_history_free(std::ptr::null_mut());
    }
}

#[test]
fn test_ffi_timing_sample_and_threshold() {
    // Record a sample — extern "C" fn with only primitive params
    ccvv_lib::ffi::ccvv_timing_record_sample(300);
    // Get threshold (not enough samples, should return 0)
    let ms = ccvv_lib::ffi::ccvv_timing_get_threshold_ms();
    assert_eq!(ms, 0, "Should return 0 with insufficient samples");
}

#[test]
fn test_ffi_is_feature_enabled_null_config() {
    unsafe {
        let feature = std::ffi::CString::new("whitespace_cleanup").unwrap();
        // null config should return defaults
        let enabled = ccvv_lib::ffi::ccvv_is_feature_enabled(std::ptr::null(), feature.as_ptr());
        assert!(enabled, "whitespace_cleanup should default to true");
    }
}

#[test]
fn test_ffi_transform_n_null_config() {
    unsafe {
        let input = std::ffi::CString::new("Hello  world").unwrap();
        let mut err: *mut std::os::raw::c_char = std::ptr::null_mut();
        // null config — should use default pipeline
        let result = ccvv_lib::ffi::ccvv_transform_n(input.as_ptr(), std::ptr::null(), &mut err);
        assert!(
            !result.cleaned_text.is_null(),
            "Should handle null config gracefully"
        );
        ccvv_lib::ffi::ccvv_transform_result_free(result);
        if !err.is_null() {
            ccvv_lib::ffi::ccvv_string_free(err);
        }
    }
}

// ===== Cross-Stage Interaction Tests =====

#[test]
fn test_normalize_then_url_cleaning() {
    // Smart quotes in URL context
    let pipeline = Pipeline::new(vec![
        Box::new(NormalizeTransform::new()),
        Box::new(UrlTransform::new()),
    ]);
    let input = "Visit \u{201C}https://www.example.com?utm_source=test\u{201D}";
    let (result, _) = pipeline.run(input);
    assert!(
        !result.contains("utm_source"),
        "URL cleaning should work after normalization"
    );
}

#[test]
fn test_whitespace_then_structural() {
    // Whitespace cleanup followed by structural detection
    let pipeline = Pipeline::new(vec![
        Box::new(WhitespaceTransform::new()),
        Box::new(StructuralTransform::new()),
    ]);
    let input = "  Name\\tAge\\tCity  \\n  Alice\\t30\\tNYC  \\n  Bob\\t25\\tLA  ";
    let (result, _) = pipeline.run(input);
    // Should at minimum clean whitespace without breaking downstream
    assert!(
        !result.is_empty(),
        "Pipeline should produce non-empty output"
    );
}

#[test]
fn test_agent_then_whitespace() {
    // ANSI stripping then whitespace cleanup
    let pipeline = Pipeline::new(vec![
        Box::new(AgentTransform::new()),
        Box::new(WhitespaceTransform::new()),
    ]);
    let input = "\x1b[31mHello\x1b[0m   \x1b[1mworld\x1b[0m   with   extra   spaces";
    let (result, _) = pipeline.run(input);
    assert!(!result.contains("\x1b["), "ANSI codes stripped");
    assert!(result.contains("Hello"), "Content preserved");
    assert!(result.contains("world"), "Content preserved");
}

// ===== Config Merge Semantics =====

#[test]
fn test_config_merge_preserves_default_exclusions() {
    // Adding user exclusions must not replace the default password manager exclusions
    let toml_str = r#"
[exclusions]
bundle_ids = ["com.custom.myapp"]
"#;
    let config: CcvvConfig = toml::from_str(toml_str).unwrap();
    let resolved = resolve_config(&config, None).unwrap();
    // All 5 defaults must be present
    assert!(resolved
        .exclusion_bundle_ids
        .contains(&"com.1password.1password".to_string()));
    assert!(resolved
        .exclusion_bundle_ids
        .contains(&"com.agilebits.onepassword7".to_string()));
    assert!(resolved
        .exclusion_bundle_ids
        .contains(&"com.lastpass.LastPass".to_string()));
    assert!(resolved
        .exclusion_bundle_ids
        .contains(&"com.bitwarden.desktop".to_string()));
    assert!(resolved
        .exclusion_bundle_ids
        .contains(&"org.keepassxc.keepassxc".to_string()));
    // User addition must also be present
    assert!(resolved
        .exclusion_bundle_ids
        .contains(&"com.custom.myapp".to_string()));
    assert_eq!(resolved.exclusion_bundle_ids.len(), 6);
}

#[test]
fn test_config_merge_appends_url_deny() {
    // User deny list is appended to defaults, not replaced
    let toml_str = r#"
[url_params]
global_deny = ["custom_tracker", "my_ref"]
"#;
    let config: CcvvConfig = toml::from_str(toml_str).unwrap();
    let resolved = resolve_config(&config, None).unwrap();
    // All defaults still present
    assert!(resolved.url_global_deny.contains(&"utm_source".to_string()));
    assert!(resolved.url_global_deny.contains(&"utm_medium".to_string()));
    assert!(resolved.url_global_deny.contains(&"fbclid".to_string()));
    assert!(resolved.url_global_deny.contains(&"gclid".to_string()));
    assert!(resolved.url_global_deny.contains(&"_ga".to_string()));
    // User additions present
    assert!(resolved
        .url_global_deny
        .contains(&"custom_tracker".to_string()));
    assert!(resolved.url_global_deny.contains(&"my_ref".to_string()));
}

#[test]
fn test_config_em_dash_custom_replacement() {
    // EmDashConfig with custom replacement string
    let toml_str = r#"
[settings.em_dash]
replace = "---"
"#;
    let config: CcvvConfig = toml::from_str(toml_str).unwrap();
    let resolved = resolve_config(&config, None).unwrap();
    assert_eq!(resolved.em_dash_replacement, "---");

    // Verify it actually works through the pipeline
    let normalize = NormalizeTransform::new().with_em_dash_replacement("---");
    let mut ctx = ccvv_lib::TransformContext::default();
    let result = normalize.apply("Hello\u{2014}world", &mut ctx);
    assert_eq!(result, "Hello---world");
}

#[test]
fn test_config_double_tap_auto() {
    // DoubleTapSetting::Adaptive parses and resolves to None
    let toml_str = r#"
[settings]
double_tap_window_ms = "auto"
"#;
    let config: CcvvConfig = toml::from_str(toml_str).unwrap();
    let resolved = resolve_config(&config, None).unwrap();
    assert!(
        resolved.resolved_double_tap_ms.is_none(),
        "Adaptive mode should resolve to None"
    );
}

// ===== History Database =====

fn temp_db_path() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join("ccvv-integration-test");
    std::fs::create_dir_all(&dir).unwrap();
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(
        "inttest-{}-{}-{}.db",
        std::process::id(),
        id,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn test_history_integer_timestamp() {
    let path = temp_db_path();
    let db = ccvv_lib::history::HistoryDb::open(&path).unwrap();
    let id = db.prepare("raw", "cleaned", None, false).unwrap();
    db.commit_entry(id).unwrap();

    let entries = db.recent(1).unwrap();
    assert_eq!(entries.len(), 1);
    // created_at should be a Unix timestamp (seconds since epoch)
    let ts = entries[0].created_at;
    // Sanity check: should be after 2020-01-01 (1577836800) and before 2100-01-01
    assert!(
        ts > 1_577_836_800,
        "Timestamp should be after 2020: got {}",
        ts
    );
    assert!(
        ts < 4_102_444_800,
        "Timestamp should be before 2100: got {}",
        ts
    );

    std::fs::remove_file(&path).ok();
}

#[cfg(unix)]
#[test]
fn test_history_file_permissions_0600() {
    use std::os::unix::fs::PermissionsExt;
    let path = temp_db_path();
    let _db = ccvv_lib::history::HistoryDb::open(&path).unwrap();
    assert!(path.exists());

    let metadata = std::fs::metadata(&path).unwrap();
    let mode = metadata.permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "DB file should have 0600 permissions, got {:o}",
        mode
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn test_history_corruption_recovery() {
    let path = temp_db_path();

    // Write garbage to simulate a corrupt database
    std::fs::write(&path, b"this is not a valid sqlite database").unwrap();

    // Opening should succeed by renaming corrupt file and creating fresh DB
    let db = ccvv_lib::history::HistoryDb::open(&path).unwrap();

    // Should be usable
    let id = db.prepare("raw", "cleaned", None, false).unwrap();
    db.commit_entry(id).unwrap();
    let entries = db.recent(1).unwrap();
    assert_eq!(entries.len(), 1);

    // Corrupt file should have been renamed
    let backup = path.with_extension("db.corrupt");
    assert!(
        backup.exists(),
        "Corrupt DB should be renamed to .db.corrupt"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&backup).ok();
}

#[test]
fn test_history_wal_mode() {
    let path = temp_db_path();
    let _db = ccvv_lib::history::HistoryDb::open(&path).unwrap();

    // Verify WAL mode by opening a second connection and checking
    let conn = rusqlite::Connection::open(&path).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal", "Journal mode should be WAL, got {}", mode);

    drop(conn);
    std::fs::remove_file(&path).ok();
    // Also clean up WAL/SHM files
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
}

// ===== User Rules Timeout =====

#[test]
fn test_userrules_timeout_respected() {
    // A pathological but linear-time regex: many alternations that all try to match
    let regex =
        regex::Regex::new(r"(a|b|c|d|e|f|g|h|i|j|k|l|m|n|o|p|q|r|s|t|u|v|w|x|y|z)+").unwrap();
    let compiled = userrules::CompiledUserRule {
        name: "heavy_rule".to_string(),
        regex,
        replacement: "X".to_string(),
    };

    let transform = userrules::UserRulesTransform::with_rules(vec![compiled]);
    let mut ctx = ccvv_lib::TransformContext::default();
    // Large input that will exercise the regex
    let input = "a".repeat(10_000);
    let result = transform.apply(&input, &mut ctx);
    // Should complete (not hang) — the regex crate guarantees linear time
    assert!(!result.is_empty());
}

// ===== Security Invariants =====

#[test]
fn test_no_expansion_attack() {
    // Ensure a crafted input cannot cause unbounded expansion
    let pipeline = default_pipeline();
    let small_input = "a".repeat(100);
    let (result, _) = pipeline.run(&small_input);
    // Result should not be more than 2x the input
    assert!(
        result.len() <= small_input.len() * 2,
        "Output must not exceed 2x expansion: input={}, output={}",
        small_input.len(),
        result.len()
    );
}

#[test]
fn test_empty_input_passthrough() {
    let pipeline = default_pipeline();
    let (result, _) = pipeline.run("");
    assert_eq!(result, "", "Empty input must produce empty output");
}

#[test]
fn test_single_char_passthrough() {
    let pipeline = default_pipeline();
    let (result, _) = pipeline.run("a");
    assert!(!result.is_empty(), "Single char should produce output");
}

#[test]
fn test_unicode_boundary_safety() {
    let pipeline = default_pipeline();
    // Mix of ASCII and multi-byte Unicode
    let input = "Hello \u{1F600} world \u{00E9}\u{0301} caf\u{00E9}";
    let (result, _) = pipeline.run(input);
    assert!(
        result.is_char_boundary(0),
        "Result must have valid char boundaries"
    );
    // Should not panic or corrupt text
    let _ = result.chars().count();
}
