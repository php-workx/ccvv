mod common;

use ccvv_linux::clipboard::gnome::{detect_limited_mode, LimitedMode};
use ccvv_linux::hotkey::{HotkeyBackend, HotkeyError, HotkeyRegistration, PortalHotkey};
use std::sync::{LazyLock, Mutex, MutexGuard};

static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
    if let Some(value) = value {
        std::env::set_var(key, value);
    } else {
        std::env::remove_var(key);
    }
}

#[test]
fn gnome_limited_integration_smoke_is_gated_by_environment() {
    if common::has_wayland_display()
        && std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase()
            .contains("gnome")
    {
        println!("gnome-limited environment appears available");
    } else {
        println!("{}", common::skip_message("gnome-limited"));
    }
}

#[test]
fn portal_hotkey_returns_unavailable_without_desktop() {
    let _guard = env_lock();
    let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
    let old_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
    let old_force = std::env::var_os("CCVV_PORTAL_FORCE_AVAILABLE");
    std::env::remove_var("XDG_CURRENT_DESKTOP");
    std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
    std::env::set_var("CCVV_PORTAL_FORCE_AVAILABLE", "false");

    let mut hotkey = PortalHotkey::new();
    let result = hotkey.register().expect("register should not error");
    assert_eq!(result, HotkeyRegistration::Unavailable);

    restore_env("XDG_CURRENT_DESKTOP", old_desktop);
    restore_env("DBUS_SESSION_BUS_ADDRESS", old_bus);
    restore_env("CCVV_PORTAL_FORCE_AVAILABLE", old_force);
}

#[test]
fn portal_hotkey_wait_before_register_errors() {
    let mut hotkey = PortalHotkey::new();
    let err = hotkey.wait_for_activation().unwrap_err();
    assert!(
        matches!(err, HotkeyError::Unavailable),
        "expected Unavailable, got: {err}"
    );
}

#[test]
fn gnome_limited_mode_defaults_to_cli_only_without_desktop_hint() {
    let _guard = env_lock();
    let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
    let old_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
    let old_force = std::env::var_os("CCVV_PORTAL_FORCE_AVAILABLE");
    std::env::remove_var("XDG_CURRENT_DESKTOP");
    std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
    std::env::set_var("CCVV_PORTAL_FORCE_AVAILABLE", "false");
    assert_eq!(detect_limited_mode(), LimitedMode::CliOnly);
    restore_env("XDG_CURRENT_DESKTOP", old_desktop);
    restore_env("DBUS_SESSION_BUS_ADDRESS", old_bus);
    restore_env("CCVV_PORTAL_FORCE_AVAILABLE", old_force);
}

#[test]
fn gnome_limited_mode_reports_hotkey_only_when_portal_probe_succeeds() {
    let _guard = env_lock();
    let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
    let old_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
    let old_force = std::env::var_os("CCVV_PORTAL_FORCE_AVAILABLE");
    std::env::set_var("XDG_CURRENT_DESKTOP", "GNOME");
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/tmp/ccvv-test-bus");
    std::env::set_var("CCVV_PORTAL_FORCE_AVAILABLE", "true");
    assert_eq!(detect_limited_mode(), LimitedMode::HotkeyOnly);
    restore_env("XDG_CURRENT_DESKTOP", old_desktop);
    restore_env("DBUS_SESSION_BUS_ADDRESS", old_bus);
    restore_env("CCVV_PORTAL_FORCE_AVAILABLE", old_force);
}

#[test]
#[cfg(target_os = "linux")]
fn gnome_limited_mode_requires_desktop_hint_even_when_probe_is_forced() {
    let _guard = env_lock();
    let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
    let old_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
    let old_force = std::env::var_os("CCVV_PORTAL_FORCE_AVAILABLE");
    std::env::remove_var("XDG_CURRENT_DESKTOP");
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/tmp/ccvv-test-bus");
    std::env::set_var("CCVV_PORTAL_FORCE_AVAILABLE", "true");

    assert_eq!(detect_limited_mode(), LimitedMode::CliOnly);

    restore_env("XDG_CURRENT_DESKTOP", old_desktop);
    restore_env("DBUS_SESSION_BUS_ADDRESS", old_bus);
    restore_env("CCVV_PORTAL_FORCE_AVAILABLE", old_force);
}

#[test]
#[cfg(target_os = "linux")]
fn gnome_limited_mode_accepts_compound_gnome_desktop_hints() {
    let _guard = env_lock();
    let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
    let old_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
    let old_force = std::env::var_os("CCVV_PORTAL_FORCE_AVAILABLE");
    std::env::set_var("XDG_CURRENT_DESKTOP", "ubuntu:GNOME");
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/tmp/ccvv-test-bus");
    std::env::set_var("CCVV_PORTAL_FORCE_AVAILABLE", "true");

    assert_eq!(detect_limited_mode(), LimitedMode::HotkeyOnly);

    restore_env("XDG_CURRENT_DESKTOP", old_desktop);
    restore_env("DBUS_SESSION_BUS_ADDRESS", old_bus);
    restore_env("CCVV_PORTAL_FORCE_AVAILABLE", old_force);
}

#[test]
#[cfg(target_os = "linux")]
fn gnome_limited_mode_requires_dbus_session_for_hotkey_only() {
    let _guard = env_lock();
    let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
    let old_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
    let old_force = std::env::var_os("CCVV_PORTAL_FORCE_AVAILABLE");
    std::env::set_var("XDG_CURRENT_DESKTOP", "GNOME");
    std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
    std::env::set_var("CCVV_PORTAL_FORCE_AVAILABLE", "true");

    assert_eq!(detect_limited_mode(), LimitedMode::CliOnly);

    restore_env("XDG_CURRENT_DESKTOP", old_desktop);
    restore_env("DBUS_SESSION_BUS_ADDRESS", old_bus);
    restore_env("CCVV_PORTAL_FORCE_AVAILABLE", old_force);
}

#[test]
#[cfg(target_os = "linux")]
fn portal_hotkey_registers_and_consumes_forced_test_trigger() {
    let _guard = env_lock();
    let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
    let old_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
    let old_force = std::env::var_os("CCVV_PORTAL_FORCE_AVAILABLE");
    let old_trigger = std::env::var_os("CCVV_PORTAL_TEST_TRIGGER");
    std::env::set_var("XDG_CURRENT_DESKTOP", "GNOME");
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/tmp/ccvv-test-bus");
    std::env::set_var("CCVV_PORTAL_FORCE_AVAILABLE", "true");
    std::env::set_var("CCVV_PORTAL_TEST_TRIGGER", "true");

    let mut hotkey = PortalHotkey::new();
    let registration = hotkey.register().expect("register should not error");
    assert_eq!(registration, HotkeyRegistration::Registered);
    assert!(hotkey.wait_for_activation().is_ok());

    restore_env("XDG_CURRENT_DESKTOP", old_desktop);
    restore_env("DBUS_SESSION_BUS_ADDRESS", old_bus);
    restore_env("CCVV_PORTAL_FORCE_AVAILABLE", old_force);
    restore_env("CCVV_PORTAL_TEST_TRIGGER", old_trigger);
}

#[test]
#[ignore = "requires GNOME session with portal availability"]
fn gnome_limited_hotkey_probe() {
    if !common::has_wayland_display() {
        return;
    }
    let mut hotkey = PortalHotkey::new();
    let result = hotkey
        .register()
        .expect("register should not error on GNOME");
    println!("hotkey registration result: {result:?}");
}
