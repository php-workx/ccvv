//! C FFI surface for Swift integration.
//!
//! All extern "C" functions that the macOS Swift app calls.
//! See §8 of the technical spec.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::sync::Mutex;

use crate::config::{load_config, resolve_config, ResolvedConfig};
use crate::history::HistoryDb;
use crate::pipeline::Pipeline;
use crate::transforms::agent::AgentTransform;
use crate::transforms::autowrap::AutowrapTransform;
use crate::transforms::normalize::NormalizeTransform;
use crate::transforms::structural::StructuralTransform;
use crate::transforms::url::UrlTransform;
use crate::transforms::userrules::UserRulesTransform;
use crate::transforms::whitespace::WhitespaceTransform;
use crate::transforms::Transform;

/// Opaque handle to a loaded config.
pub struct CcvvConfig {
    resolved: ResolvedConfig,
}

/// Opaque handle to a history database.
pub struct CcvvHistory {
    db: HistoryDb,
}

// --- String helpers ---

/// Free a string allocated by ccvv.
///
/// # Safety
/// `s` must be a pointer returned by a ccvv function, or null.
#[no_mangle]
pub unsafe extern "C" fn ccvv_string_free(s: *mut c_char) {
    if !s.is_null() {
        let _ = unsafe { CString::from_raw(s) };
    }
}

/// Helper: convert C string to Rust &str. Returns None on null or invalid UTF-8.
unsafe fn cstr_to_str<'a>(s: *const c_char) -> Option<&'a str> {
    if s.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(s) }.to_str().ok()
}

/// Helper: set error message via error_out pointer.
unsafe fn set_error(error_out: *mut *mut c_char, msg: &str) {
    if !error_out.is_null() {
        if let Ok(c_msg) = CString::new(msg) {
            unsafe { *error_out = c_msg.into_raw() };
        }
    }
}

/// Helper: convert Rust string to C string. Returns null on failure.
fn to_c_string(s: &str) -> *mut c_char {
    CString::new(s).map(|cs| cs.into_raw()).unwrap_or(ptr::null_mut())
}

// --- Transform functions ---

/// Transform text using the default pipeline configuration.
///
/// Returns a newly allocated C string with the cleaned text.
/// Caller must free with `ccvv_string_free`.
///
/// # Safety
/// `input` must be a valid null-terminated C string or null.
/// `error_out` may be null.
#[no_mangle]
pub unsafe extern "C" fn ccvv_transform(
    input: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    let Some(text) = (unsafe { cstr_to_str(input) }) else {
        unsafe { set_error(error_out, "null input") };
        return ptr::null_mut();
    };

    let pipeline = build_default_pipeline();
    let (result, _ctx) = pipeline.run(text);
    to_c_string(&result)
}

/// Transform text using a loaded config.
///
/// # Safety
/// `input` and `config` must be valid pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ccvv_transform_n(
    input: *const c_char,
    config: *const CcvvConfig,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    let Some(text) = (unsafe { cstr_to_str(input) }) else {
        unsafe { set_error(error_out, "null input") };
        return ptr::null_mut();
    };

    let pipeline = if config.is_null() {
        build_default_pipeline()
    } else {
        let cfg = unsafe { &*config };
        build_pipeline_from_config(&cfg.resolved)
    };

    let (result, _ctx) = pipeline.run(text);
    to_c_string(&result)
}

/// Free a transform result string. Alias for `ccvv_string_free`.
///
/// # Safety
/// `result` must be a pointer returned by `ccvv_transform` or null.
#[no_mangle]
pub unsafe extern "C" fn ccvv_transform_result_free(result: *mut c_char) {
    unsafe { ccvv_string_free(result) };
}

// --- Config functions ---

/// Load configuration from a TOML file.
///
/// If `path` is null, loads from the default path (`~/.ccvv/config.toml`).
/// Returns an opaque config handle. Free with `ccvv_config_free`.
///
/// # Safety
/// `path` must be a valid C string or null.
#[no_mangle]
pub unsafe extern "C" fn ccvv_load_config(
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut CcvvConfig {
    let path_opt = unsafe { cstr_to_str(path) };
    let path_buf = path_opt.map(std::path::PathBuf::from);

    let config = match load_config(path_buf.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            unsafe { set_error(error_out, &e.to_string()) };
            return ptr::null_mut();
        }
    };

    let resolved = match resolve_config(&config, None) {
        Ok(r) => r,
        Err(e) => {
            unsafe { set_error(error_out, &e.to_string()) };
            return ptr::null_mut();
        }
    };

    Box::into_raw(Box::new(CcvvConfig { resolved }))
}

/// Free a config handle.
///
/// # Safety
/// `config` must be a pointer returned by `ccvv_load_config` or null.
#[no_mangle]
pub unsafe extern "C" fn ccvv_config_free(config: *mut CcvvConfig) {
    if !config.is_null() {
        let _ = unsafe { Box::from_raw(config) };
    }
}

/// Get the double-tap window in milliseconds from config.
///
/// # Safety
/// `config` must be a valid pointer or null (returns default 450).
#[no_mangle]
pub unsafe extern "C" fn ccvv_get_double_tap_window_ms(config: *const CcvvConfig) -> u32 {
    if config.is_null() {
        return 450;
    }
    unsafe { &*config }.resolved.settings.double_tap_window_ms
}

/// Check if a feature is enabled in the config.
///
/// # Safety
/// `config` and `feature_name` must be valid pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ccvv_is_feature_enabled(
    config: *const CcvvConfig,
    feature_name: *const c_char,
) -> bool {
    let Some(name) = (unsafe { cstr_to_str(feature_name) }) else {
        return false;
    };
    let settings = if config.is_null() {
        crate::config::Settings::default()
    } else {
        unsafe { &*config }.resolved.settings.clone()
    };

    match name {
        "normalize_unicode" => settings.normalize_unicode,
        "whitespace_cleanup" => settings.whitespace_cleanup,
        "agent_strip" => settings.agent_strip,
        "structural_detection" => settings.structural_detection,
        "url_cleaning" => settings.url_cleaning,
        "auto_wrapper" => settings.auto_wrapper,
        "user_rules" => settings.user_rules,
        "sensitive_filter" => settings.sensitive_filter,
        _ => false,
    }
}

/// Check if an app is excluded by bundle ID.
///
/// # Safety
/// `config` and `bundle_id` must be valid pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ccvv_is_app_excluded(
    config: *const CcvvConfig,
    bundle_id: *const c_char,
) -> bool {
    let Some(bid) = (unsafe { cstr_to_str(bundle_id) }) else {
        return false;
    };
    if config.is_null() {
        return false;
    }
    let cfg = unsafe { &*config };
    cfg.resolved.exclusion_bundle_ids.iter().any(|b| b == bid)
}

/// Validate a config file. Returns null on success, or error string on failure.
///
/// # Safety
/// `path` must be a valid C string or null.
#[no_mangle]
pub unsafe extern "C" fn ccvv_validate_config(
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> bool {
    let path_opt = unsafe { cstr_to_str(path) };
    let path_buf = path_opt.map(std::path::PathBuf::from);

    let config = match load_config(path_buf.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            unsafe { set_error(error_out, &e.to_string()) };
            return false;
        }
    };

    let errors = crate::config::validate_config(&config);
    if errors.is_empty() {
        true
    } else {
        unsafe { set_error(error_out, &errors.join("\n")) };
        false
    }
}

// --- Config write functions (pre-mortem fix: BLOCKER 2) ---

/// Set a boolean config key. Uses toml_edit for round-trip safe writes.
///
/// # Safety
/// `key`, `config_path` must be valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ccvv_config_set_bool(
    key: *const c_char,
    value: bool,
    config_path: *const c_char,
    error_out: *mut *mut c_char,
) -> bool {
    let Some(key_str) = (unsafe { cstr_to_str(key) }) else {
        unsafe { set_error(error_out, "null key") };
        return false;
    };
    let Some(path_str) = (unsafe { cstr_to_str(config_path) }) else {
        unsafe { set_error(error_out, "null config path") };
        return false;
    };

    let path = std::path::Path::new(path_str);
    let content = if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                unsafe { set_error(error_out, &format!("read error: {}", e)) };
                return false;
            }
        }
    } else {
        String::new()
    };

    let mut doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(e) => {
            unsafe { set_error(error_out, &format!("toml parse error: {}", e)) };
            return false;
        }
    };

    // Ensure [settings] table exists
    if doc.get("settings").is_none() {
        doc["settings"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    doc["settings"][key_str] = toml_edit::value(value);

    match std::fs::write(path, doc.to_string()) {
        Ok(()) => true,
        Err(e) => {
            unsafe { set_error(error_out, &format!("write error: {}", e)) };
            false
        }
    }
}

/// Set a string config key. Uses toml_edit for round-trip safe writes.
///
/// # Safety
/// `key`, `value`, `config_path` must be valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ccvv_config_set_string(
    key: *const c_char,
    value: *const c_char,
    config_path: *const c_char,
    error_out: *mut *mut c_char,
) -> bool {
    let Some(key_str) = (unsafe { cstr_to_str(key) }) else {
        unsafe { set_error(error_out, "null key") };
        return false;
    };
    let Some(value_str) = (unsafe { cstr_to_str(value) }) else {
        unsafe { set_error(error_out, "null value") };
        return false;
    };
    let Some(path_str) = (unsafe { cstr_to_str(config_path) }) else {
        unsafe { set_error(error_out, "null config path") };
        return false;
    };

    let path = std::path::Path::new(path_str);
    let content = if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                unsafe { set_error(error_out, &format!("read error: {}", e)) };
                return false;
            }
        }
    } else {
        String::new()
    };

    let mut doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(e) => {
            unsafe { set_error(error_out, &format!("toml parse error: {}", e)) };
            return false;
        }
    };

    if doc.get("settings").is_none() {
        doc["settings"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    doc["settings"][key_str] = toml_edit::value(value_str);

    match std::fs::write(path, doc.to_string()) {
        Ok(()) => true,
        Err(e) => {
            unsafe { set_error(error_out, &format!("write error: {}", e)) };
            false
        }
    }
}

// --- History functions ---

/// Open the history database.
///
/// # Safety
/// `path` must be a valid C string or null (uses default path).
#[no_mangle]
pub unsafe extern "C" fn ccvv_history_open(
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut CcvvHistory {
    let db_path = if let Some(p) = unsafe { cstr_to_str(path) } {
        std::path::PathBuf::from(p)
    } else {
        let home = match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => {
                unsafe { set_error(error_out, "HOME not set") };
                return ptr::null_mut();
            }
        };
        std::path::PathBuf::from(home)
            .join(".ccvv")
            .join("history.db")
    };

    match HistoryDb::open(&db_path) {
        Ok(db) => Box::into_raw(Box::new(CcvvHistory { db })),
        Err(e) => {
            unsafe { set_error(error_out, &e.to_string()) };
            ptr::null_mut()
        }
    }
}

/// Prepare a history entry (two-phase commit step 1).
///
/// # Safety
/// All pointers must be valid or null where indicated.
#[no_mangle]
pub unsafe extern "C" fn ccvv_history_prepare(
    history: *mut CcvvHistory,
    raw_text: *const c_char,
    cleaned_text: *const c_char,
    store_raw: bool,
    error_out: *mut *mut c_char,
) -> i64 {
    if history.is_null() {
        unsafe { set_error(error_out, "null history handle") };
        return -1;
    }
    let Some(raw) = (unsafe { cstr_to_str(raw_text) }) else {
        unsafe { set_error(error_out, "null raw_text") };
        return -1;
    };
    let Some(cleaned) = (unsafe { cstr_to_str(cleaned_text) }) else {
        unsafe { set_error(error_out, "null cleaned_text") };
        return -1;
    };

    let h = unsafe { &*history };
    match h.db.prepare(raw, cleaned, None, store_raw) {
        Ok(id) => id,
        Err(e) => {
            unsafe { set_error(error_out, &e.to_string()) };
            -1
        }
    }
}

/// Commit a history entry (two-phase commit step 2).
///
/// # Safety
/// `history` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ccvv_history_commit(
    history: *mut CcvvHistory,
    entry_id: i64,
    error_out: *mut *mut c_char,
) -> bool {
    if history.is_null() {
        unsafe { set_error(error_out, "null history handle") };
        return false;
    }
    let h = unsafe { &*history };
    match h.db.commit_entry(entry_id) {
        Ok(()) => true,
        Err(e) => {
            unsafe { set_error(error_out, &e.to_string()) };
            false
        }
    }
}

/// Rollback an uncommitted history entry.
///
/// # Safety
/// `history` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ccvv_history_rollback(
    history: *mut CcvvHistory,
    entry_id: i64,
    error_out: *mut *mut c_char,
) -> bool {
    if history.is_null() {
        unsafe { set_error(error_out, "null history handle") };
        return false;
    }
    let h = unsafe { &*history };
    match h.db.rollback_entry(entry_id) {
        Ok(()) => true,
        Err(e) => {
            unsafe { set_error(error_out, &e.to_string()) };
            false
        }
    }
}

/// Get the raw text for the most recent undo.
///
/// # Safety
/// `history` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ccvv_history_undo_raw(
    history: *mut CcvvHistory,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    if history.is_null() {
        unsafe { set_error(error_out, "null history handle") };
        return ptr::null_mut();
    }
    let h = unsafe { &*history };
    match h.db.undo_raw() {
        Some(raw) => to_c_string(&raw),
        None => {
            unsafe { set_error(error_out, "no undo available") };
            ptr::null_mut()
        }
    }
}

/// Get recent history as JSON.
///
/// # Safety
/// `history` must be a valid pointer.
#[no_mangle]
pub unsafe extern "C" fn ccvv_history_get_recent_json(
    history: *mut CcvvHistory,
    limit: i32,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    if history.is_null() {
        unsafe { set_error(error_out, "null history handle") };
        return ptr::null_mut();
    }
    let h = unsafe { &*history };
    match h.db.recent(limit.max(1) as usize) {
        Ok(entries) => {
            let json_entries: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id,
                        "preview": e.preview,
                        "content_type": e.content_type,
                        "created_at": e.created_at,
                    })
                })
                .collect();
            let json = serde_json::to_string(&json_entries).unwrap_or_default();
            to_c_string(&json)
        }
        Err(e) => {
            unsafe { set_error(error_out, &e.to_string()) };
            ptr::null_mut()
        }
    }
}

/// Search history as JSON.
///
/// # Safety
/// `history` and `query` must be valid pointers.
#[no_mangle]
pub unsafe extern "C" fn ccvv_history_search_json(
    history: *mut CcvvHistory,
    query: *const c_char,
    limit: i32,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    if history.is_null() {
        unsafe { set_error(error_out, "null history handle") };
        return ptr::null_mut();
    }
    let Some(q) = (unsafe { cstr_to_str(query) }) else {
        unsafe { set_error(error_out, "null query") };
        return ptr::null_mut();
    };
    let h = unsafe { &*history };
    match h.db.search(q, limit.max(1) as usize) {
        Ok(entries) => {
            let json_entries: Vec<serde_json::Value> = entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id,
                        "preview": e.preview,
                        "cleaned_text": e.cleaned_text,
                        "content_type": e.content_type,
                        "created_at": e.created_at,
                    })
                })
                .collect();
            let json = serde_json::to_string(&json_entries).unwrap_or_default();
            to_c_string(&json)
        }
        Err(e) => {
            unsafe { set_error(error_out, &e.to_string()) };
            ptr::null_mut()
        }
    }
}

/// Free a history handle.
///
/// # Safety
/// `history` must be a pointer returned by `ccvv_history_open` or null.
#[no_mangle]
pub unsafe extern "C" fn ccvv_history_free(history: *mut CcvvHistory) {
    if !history.is_null() {
        let _ = unsafe { Box::from_raw(history) };
    }
}

// --- Timing functions (pre-mortem fix: WARN) ---

/// Thread-safe timing state.
static TIMING_SAMPLES: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Record a timing sample (interval in ms between double-taps).
#[no_mangle]
pub extern "C" fn ccvv_timing_record_sample(interval_ms: u32) {
    if let Ok(mut samples) = TIMING_SAMPLES.lock() {
        samples.push(interval_ms);
        // Keep only last 100 samples
        if samples.len() > 100 {
            let excess = samples.len() - 100;
            samples.drain(..excess);
        }
    }
}

/// Get the adaptive threshold in ms based on recorded samples.
/// Returns 0 if not enough samples to compute (falls back to config).
#[no_mangle]
pub extern "C" fn ccvv_timing_get_threshold_ms() -> u32 {
    if let Ok(samples) = TIMING_SAMPLES.lock() {
        if samples.len() < 10 {
            return 0;
        }
        // Use 90th percentile of recorded intervals
        let mut sorted: Vec<u32> = samples.clone();
        sorted.sort();
        let idx = (sorted.len() * 90) / 100;
        sorted.get(idx).copied().unwrap_or(0)
    } else {
        0
    }
}

// --- Pipeline builder helpers ---

/// Build a pipeline with all stages enabled (default config).
fn build_default_pipeline() -> Pipeline {
    let stages: Vec<Box<dyn Transform>> = vec![
        Box::new(NormalizeTransform::new()),
        Box::new(WhitespaceTransform::new()),
        Box::new(AgentTransform::new()),
        Box::new(StructuralTransform::new()),
        Box::new(UrlTransform::new()),
        // AutowrapTransform disabled by default
        // UserRulesTransform has no rules by default
    ];
    Pipeline::new(stages)
}

/// Build a pipeline from resolved config.
fn build_pipeline_from_config(config: &ResolvedConfig) -> Pipeline {
    let mut stages: Vec<Box<dyn Transform>> = Vec::new();

    if config.settings.normalize_unicode {
        stages.push(Box::new(
            NormalizeTransform::new()
                .with_em_dash_replacement(config.settings.em_dash_replace),
        ));
    }
    if config.settings.whitespace_cleanup {
        stages.push(Box::new(WhitespaceTransform::new()));
    }
    if config.settings.agent_strip {
        stages.push(Box::new(AgentTransform::new()));
    }
    if config.settings.structural_detection {
        stages.push(Box::new(StructuralTransform::new()));
    }
    if config.settings.url_cleaning {
        stages.push(Box::new(
            UrlTransform::new().with_strip_scheme(config.settings.url_strip_scheme),
        ));
    }
    if config.settings.auto_wrapper {
        stages.push(Box::new(AutowrapTransform::new()));
    }
    if config.settings.user_rules && !config.compiled_rules.is_empty() {
        stages.push(Box::new(UserRulesTransform::with_rules(
            config.compiled_rules.clone(),
        )));
    }

    Pipeline::new(stages)
        .with_max_input_bytes(config.settings.max_input_bytes)
        .with_sensitive_filter(config.settings.sensitive_filter)
}
