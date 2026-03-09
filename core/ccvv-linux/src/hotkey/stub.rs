use super::{HotkeyBackend, HotkeyError, HotkeyRegistration};

/// Stub hotkey backend for non-Linux platforms.
#[derive(Debug)]
pub struct PortalHotkey;

impl PortalHotkey {
    pub fn new() -> Self {
        Self
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
