use super::{HotkeyBackend, HotkeyError, HotkeyRegistration};

/// Stub hotkey backend for non-Linux platforms.
#[derive(Debug)]
pub struct PortalHotkey;

impl PortalHotkey {
    pub fn new() -> Self {
        Self
    }

    pub fn is_available() -> bool {
        if let Some(forced) = Self::forced_probe_result() {
            return forced;
        }

        Self::desktop_hint_supported() && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
    }

    fn desktop_hint_supported() -> bool {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let desktop_lower = desktop.to_lowercase();

        desktop_lower.contains("gnome")
            || desktop_lower.contains("kde")
            || desktop_lower.contains("plasma")
    }

    fn forced_probe_result() -> Option<bool> {
        match std::env::var("CCVV_PORTAL_FORCE_AVAILABLE") {
            Ok(value) => match value.to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "available" => Some(true),
                "0" | "false" | "no" | "unavailable" => Some(false),
                _ => None,
            },
            Err(_) => None,
        }
    }
}

impl Default for PortalHotkey {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyBackend for PortalHotkey {
    fn register(&mut self) -> Result<HotkeyRegistration, HotkeyError> {
        Ok(HotkeyRegistration::Unavailable)
    }

    fn wait_for_activation(&mut self) -> Result<(), HotkeyError> {
        Err(HotkeyError::Unavailable)
    }
}
