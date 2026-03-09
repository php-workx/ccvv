use super::{HotkeyBackend, HotkeyError, HotkeyRegistration};

/// GNOME GlobalShortcuts portal hotkey backend.
///
/// Uses the `org.freedesktop.portal.GlobalShortcuts` D-Bus interface
/// to register a "clean clipboard" action. Requires a compositor
/// that supports this portal (GNOME 44+, KDE Plasma 6+).
///
/// Full D-Bus integration requires zbus; this module currently probes
/// for portal availability and returns [`HotkeyRegistration::Unavailable`]
/// until the D-Bus dependency is added.
#[derive(Debug)]
pub struct PortalHotkey {
    registered: bool,
}

impl PortalHotkey {
    pub fn new() -> Self {
        Self { registered: false }
    }

    /// Check if the GlobalShortcuts portal is likely available based on
    /// the current desktop environment.
    fn probe_portal() -> bool {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let desktop_lower = desktop.to_lowercase();
        desktop_lower.contains("gnome")
            || desktop_lower.contains("kde")
            || desktop_lower.contains("plasma")
    }
}

impl Default for PortalHotkey {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyBackend for PortalHotkey {
    fn register(&mut self) -> Result<HotkeyRegistration, HotkeyError> {
        if !Self::probe_portal() {
            return Ok(HotkeyRegistration::Unavailable);
        }

        // TODO: Full implementation requires zbus for D-Bus communication.
        // The portal interface is org.freedesktop.portal.GlobalShortcuts:
        //   CreateSession() → session handle
        //   BindShortcuts(session, shortcuts, parent_window) → bound shortcuts
        //   ListShortcuts(session) → current bindings
        //   Signal: Activated(session_handle, shortcut_id, timestamp, options)
        //
        // For v1, we detect portal availability but defer full registration
        // until zbus is added as a dependency.

        Ok(HotkeyRegistration::Unavailable)
    }

    fn wait_for_activation(&mut self) -> Result<(), HotkeyError> {
        if !self.registered {
            return Err(HotkeyError::Unavailable);
        }
        Err(HotkeyError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_hotkey_returns_unavailable_without_portal() {
        let mut hotkey = PortalHotkey::new();
        let result = hotkey.register().unwrap();
        // Without a real desktop environment, probe_portal returns false
        // or the portal itself is not available
        assert_eq!(result, HotkeyRegistration::Unavailable);
    }

    #[test]
    fn wait_before_register_returns_error() {
        let mut hotkey = PortalHotkey::new();
        assert!(hotkey.wait_for_activation().is_err());
    }
}
