mod common;

#[test]
fn gnome_limited_integration_smoke_is_gated_by_environment() {
    if common::has_wayland_display()
        && std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_else(|_| String::new())
            .to_lowercase()
            .contains("gnome")
    {
        println!("gnome-limited environment appears available");
    } else {
        println!("{}", common::skip_message("gnome-limited"));
    }
}

#[test]
#[ignore = "requires GNOME session with portal availability"]
fn gnome_limited_hotkey_probe() {
    // TODO: add explicit hotkey availability assertion.
}
