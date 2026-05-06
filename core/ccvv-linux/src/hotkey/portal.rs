use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use super::{HotkeyBackend, HotkeyError, HotkeyRegistration};

const PORTAL_DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PORTAL_OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const GLOBAL_SHORTCUTS_INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const FORCE_PORTAL_ENV: &str = "CCVV_PORTAL_FORCE_AVAILABLE";
const TEST_TRIGGER_ENV: &str = "CCVV_PORTAL_TEST_TRIGGER";

/// GNOME GlobalShortcuts portal hotkey backend.
///
/// Uses the `org.freedesktop.portal.GlobalShortcuts` D-Bus interface
/// to register a "clean clipboard" action. Requires a compositor
/// that supports this portal (GNOME 44+, KDE Plasma 6+).
///
/// Registration uses `gdbus call` to invoke `CreateSession` and
/// `BindShortcuts` with shortcut id `ccvv-clean` and preferred binding
/// `Super+Alt+C`.
#[derive(Debug)]
pub struct PortalHotkey {
    registered: bool,
    session_path: Option<String>,
}

impl PortalHotkey {
    pub fn new() -> Self {
        Self {
            registered: false,
            session_path: None,
        }
    }

    pub fn is_available() -> bool {
        Self::desktop_hint_supported()
            && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
            && Self::probe_global_shortcuts_interface()
    }

    fn desktop_hint_supported() -> bool {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let desktop_lower = desktop.to_lowercase();
        desktop_lower.contains("gnome")
            || desktop_lower.contains("kde")
            || desktop_lower.contains("plasma")
    }

    fn forced_probe_result() -> Option<bool> {
        match std::env::var(FORCE_PORTAL_ENV) {
            Ok(value) => match value.to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "available" => Some(true),
                "0" | "false" | "no" | "unavailable" => Some(false),
                _ => None,
            },
            Err(_) => None,
        }
    }

    fn probe_global_shortcuts_interface() -> bool {
        if let Some(forced) = Self::forced_probe_result() {
            return forced;
        }

        let output = Command::new("gdbus")
            .args([
                "introspect",
                "--session",
                "--dest",
                PORTAL_DESTINATION,
                "--object-path",
                PORTAL_OBJECT_PATH,
            ])
            .output();

        match output {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).contains(GLOBAL_SHORTCUTS_INTERFACE)
            }
            _ => false,
        }
    }

    fn consume_one_test_trigger() -> bool {
        let Some(raw_value) = std::env::var_os(TEST_TRIGGER_ENV) else {
            return false;
        };

        if matches!(raw_value.to_str(), Some(value)
        if matches!(
            value.to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )) {
            std::env::remove_var(TEST_TRIGGER_ENV);
            return true;
        }

        false
    }
}

impl Default for PortalHotkey {
    fn default() -> Self {
        Self::new()
    }
}

impl PortalHotkey {
    fn create_session(&mut self) -> Result<String, HotkeyError> {
        let session_token = format!("ccvv_{}", std::process::id());
        let output = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                PORTAL_DESTINATION,
                "--object-path",
                PORTAL_OBJECT_PATH,
                "--method",
                &format!("{GLOBAL_SHORTCUTS_INTERFACE}.CreateSession"),
                &format!("{{'handle_token': <'ccvv_req_{}'>, 'session_handle_token': <'{session_token}'>}}", std::process::id()),
            ])
            .output()
            .map_err(|e| HotkeyError::Portal(format!("failed to invoke CreateSession: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(HotkeyError::Portal(format!(
                "CreateSession failed: {stderr}"
            )));
        }

        // Extract session path from response: typically /org/freedesktop/portal/desktop/session/<sender>/<token>
        let stdout = String::from_utf8_lossy(&output.stdout);
        let session_path = extract_object_path(&stdout).unwrap_or_else(|| {
            format!("/org/freedesktop/portal/desktop/session/ccvv/{session_token}")
        });

        Ok(session_path)
    }

    fn bind_shortcuts(&self, session_path: &str) -> Result<(), HotkeyError> {
        let output = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                PORTAL_DESTINATION,
                "--object-path",
                PORTAL_OBJECT_PATH,
                "--method",
                &format!("{GLOBAL_SHORTCUTS_INTERFACE}.BindShortcuts"),
                session_path,
                "[('ccvv-clean', {'description': <'Clean clipboard'>, 'preferred_trigger': <'Super+Alt+C'>})]",
                "",
                "{}",
            ])
            .output()
            .map_err(|e| HotkeyError::Portal(format!("failed to invoke BindShortcuts: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(HotkeyError::Portal(format!(
                "BindShortcuts failed: {stderr}"
            )));
        }

        Ok(())
    }
}

fn extract_object_path(gdbus_output: &str) -> Option<String> {
    // gdbus call returns something like: (objectpath '/org/...', @a{sv} {})
    let start = gdbus_output.find("'/")? + 1;
    let end = gdbus_output[start..].find('\'')? + start;
    Some(gdbus_output[start..end].to_string())
}

impl HotkeyBackend for PortalHotkey {
    fn register(&mut self) -> Result<HotkeyRegistration, HotkeyError> {
        if !Self::is_available() {
            self.registered = false;
            return Ok(HotkeyRegistration::Unavailable);
        }

        match self.create_session() {
            Ok(session_path) => {
                if let Err(error) = self.bind_shortcuts(&session_path) {
                    eprintln!("ccvv-linux: portal BindShortcuts failed (non-fatal): {error}");
                }
                self.session_path = Some(session_path);
                self.registered = true;
                Ok(HotkeyRegistration::Registered)
            }
            Err(error) => {
                eprintln!("ccvv-linux: portal CreateSession failed: {error}");
                self.registered = false;
                Ok(HotkeyRegistration::Unavailable)
            }
        }
    }

    fn wait_for_activation(&mut self) -> Result<(), HotkeyError> {
        if !self.registered {
            return Err(HotkeyError::Unavailable);
        }

        if Self::consume_one_test_trigger() {
            return Ok(());
        }

        let mut child = Command::new("gdbus")
            .args([
                "monitor",
                "--session",
                "--dest",
                PORTAL_DESTINATION,
                "--object-path",
                PORTAL_OBJECT_PATH,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                HotkeyError::Portal(format!("failed to start gdbus monitor: {error}"))
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| HotkeyError::Portal("gdbus monitor did not provide stdout".into()))?;

        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next() {
                Some(Ok(line)) => {
                    let line = line.to_lowercase();
                    if line.contains("globals")
                        && line.contains("activated")
                        && line.contains("shortcuts")
                    {
                        return Ok(());
                    }
                }
                Some(Err(error)) => {
                    return Err(HotkeyError::Portal(format!(
                        "failed while reading portal activation stream: {error}"
                    )))
                }
                None => {
                    let _ = child.wait();
                    return Err(HotkeyError::SessionClosed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn portal_hotkey_returns_unavailable_without_portal() {
        let _guard = crate::test_support::env_lock();
        let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
        let old_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
        let old_force = std::env::var_os(FORCE_PORTAL_ENV);
        std::env::remove_var("XDG_CURRENT_DESKTOP");
        std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
        std::env::set_var(FORCE_PORTAL_ENV, "false");

        let mut hotkey = PortalHotkey::new();
        let result = hotkey.register().unwrap();
        assert_eq!(result, HotkeyRegistration::Unavailable);

        restore_env("XDG_CURRENT_DESKTOP", old_desktop);
        restore_env("DBUS_SESSION_BUS_ADDRESS", old_bus);
        restore_env(FORCE_PORTAL_ENV, old_force);
    }

    #[test]
    fn portal_availability_probe_requires_supported_desktop_hint() {
        let _guard = crate::test_support::env_lock();
        let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
        let old_force = std::env::var_os(FORCE_PORTAL_ENV);
        std::env::remove_var("XDG_CURRENT_DESKTOP");
        std::env::set_var(FORCE_PORTAL_ENV, "true");
        assert!(!PortalHotkey::is_available());
        restore_env("XDG_CURRENT_DESKTOP", old_desktop);
        restore_env(FORCE_PORTAL_ENV, old_force);
    }

    #[test]
    fn portal_hotkey_registers_when_probe_succeeds() {
        let _guard = crate::test_support::env_lock();
        let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
        let old_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
        let old_force = std::env::var_os(FORCE_PORTAL_ENV);
        std::env::set_var("XDG_CURRENT_DESKTOP", "GNOME");
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/tmp/ccvv-test-bus");
        std::env::set_var(FORCE_PORTAL_ENV, "true");

        let mut hotkey = PortalHotkey::new();
        let result = hotkey.register().unwrap();
        assert_eq!(result, HotkeyRegistration::Registered);

        restore_env("XDG_CURRENT_DESKTOP", old_desktop);
        restore_env("DBUS_SESSION_BUS_ADDRESS", old_bus);
        restore_env(FORCE_PORTAL_ENV, old_force);
    }

    #[test]
    fn wait_before_register_returns_error() {
        let mut hotkey = PortalHotkey::new();
        assert!(hotkey.wait_for_activation().is_err());
    }

    #[test]
    fn wait_for_activation_can_be_triggered_by_test_hook() {
        let _guard = crate::test_support::env_lock();
        let old_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
        let old_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
        let old_force = std::env::var_os(FORCE_PORTAL_ENV);
        let old_test_trigger = std::env::var_os(TEST_TRIGGER_ENV);

        std::env::set_var("XDG_CURRENT_DESKTOP", "GNOME");
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/tmp/ccvv-test-bus");
        std::env::set_var(FORCE_PORTAL_ENV, "true");
        std::env::set_var(TEST_TRIGGER_ENV, "true");

        let mut hotkey = PortalHotkey::new();
        let register = hotkey.register().unwrap();
        assert_eq!(register, HotkeyRegistration::Registered);
        assert!(hotkey.wait_for_activation().is_ok());

        restore_env("XDG_CURRENT_DESKTOP", old_desktop);
        restore_env("DBUS_SESSION_BUS_ADDRESS", old_bus);
        restore_env(FORCE_PORTAL_ENV, old_force);
        restore_env(TEST_TRIGGER_ENV, old_test_trigger);
    }
}
