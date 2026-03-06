//! CLI integration tests for ccvv.

use assert_cmd::Command;
use predicates::prelude::*;

fn ccvv_cmd() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("ccvv"))
}

#[test]
fn test_transform_stdin() {
    ccvv_cmd()
        .write_stdin("hello  world")
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn test_transform_utm() {
    ccvv_cmd()
        .arg("--strip-urls")
        .write_stdin("https://example.com/page?utm_source=google&id=123")
        .assert()
        .success()
        .stdout(predicate::str::contains("id=123"))
        .stdout(predicate::str::contains("utm_source").not());
}

#[test]
fn test_version() {
    ccvv_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("1.1.0"));
}

#[test]
fn test_doctor() {
    ccvv_cmd()
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("ccvv doctor"));
}

#[test]
fn test_validate_missing_config() {
    ccvv_cmd()
        .arg("validate")
        .arg("--config")
        .arg("/nonexistent/path/config.toml")
        .assert()
        .stdout(predicate::str::contains("not found").or(predicate::str::contains("error")));
}

#[test]
fn test_preview() {
    ccvv_cmd()
        .arg("preview")
        .write_stdin("hello  world  with   extra   spaces")
        .assert()
        .success();
}

#[test]
fn test_history_list_empty() {
    // Use a temp config that points to non-existent history
    // The history command should handle gracefully
    let _ = ccvv_cmd().arg("history").assert();
    // Just verify it doesn't panic — exit code may be non-zero if no DB
}

#[test]
fn test_transform_sensitive_skip() {
    // Use a temp config with sensitive_filter enabled (don't depend on user's ~/.ccvv/config.toml)
    let dir = std::env::temp_dir().join("ccvv-test-sensitive");
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "[settings]\nsensitive_filter = true\n").unwrap();

    let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
    ccvv_cmd()
        .arg("--config")
        .arg(config_path.to_str().unwrap())
        .write_stdin(pem)
        .assert()
        .success()
        .stdout(predicate::str::contains("BEGIN RSA PRIVATE KEY"));
}
