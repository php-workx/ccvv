use crate::hotkey::PortalHotkey;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitedMode {
    HotkeyOnly,
    CliOnly,
}

impl LimitedMode {
    pub fn automatic_monitoring_enabled(self) -> bool {
        false
    }

    pub fn supports_portal_hotkey(self) -> bool {
        matches!(self, Self::HotkeyOnly)
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::HotkeyOnly => "hotkey/manual only",
            Self::CliOnly => "cli only",
        }
    }
}

pub fn detect_limited_mode() -> LimitedMode {
    if PortalHotkey::is_available() {
        LimitedMode::HotkeyOnly
    } else {
        LimitedMode::CliOnly
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_limited_mode, LimitedMode};

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn limited_mode_defaults_to_cli_only_without_desktop_hint() {
        let _guard = crate::test_support::env_lock();
        let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
        let old_force = std::env::var_os("CCVV_PORTAL_FORCE_AVAILABLE");
        std::env::remove_var("XDG_CURRENT_DESKTOP");
        std::env::set_var("CCVV_PORTAL_FORCE_AVAILABLE", "false");
        assert_eq!(detect_limited_mode(), LimitedMode::CliOnly);
        restore_env("XDG_CURRENT_DESKTOP", old_desktop);
        restore_env("CCVV_PORTAL_FORCE_AVAILABLE", old_force);
    }

    #[test]
    fn limited_mode_uses_hotkey_only_for_supported_desktops() {
        let _guard = crate::test_support::env_lock();
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
    fn limited_mode_describes_variants() {
        assert_eq!(LimitedMode::HotkeyOnly.description(), "hotkey/manual only");
        assert_eq!(LimitedMode::CliOnly.description(), "cli only");
        assert!(!LimitedMode::HotkeyOnly.automatic_monitoring_enabled());
    }
}
