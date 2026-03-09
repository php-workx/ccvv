mod common;

use ccvv_linux::hotkey::{HotkeyBackend, HotkeyError, HotkeyRegistration, PortalHotkey};

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
    let mut hotkey = PortalHotkey::new();
    // Without a real GNOME/KDE desktop, registration should return Unavailable
    let result = hotkey.register().expect("register should not error");
    assert_eq!(result, HotkeyRegistration::Unavailable);
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
#[ignore = "requires GNOME session with portal availability"]
fn gnome_limited_hotkey_probe() {
    if !common::has_wayland_display() {
        return;
    }
    let mut hotkey = PortalHotkey::new();
    let result = hotkey
        .register()
        .expect("register should not error on GNOME");
    // On a real GNOME session, this may return Registered or Unavailable
    // depending on portal support. Either is valid for now.
    println!("hotkey registration result: {result:?}");
}
