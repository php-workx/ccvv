//! Hotkey detection for desktop environments that restrict global key capture.
//!
//! On GNOME Wayland, applications cannot monitor keyboard events globally.
//! Instead, the XDG GlobalShortcuts portal can register a "clean clipboard"
//! action that users bind to their preferred shortcut.

#[cfg(target_os = "linux")]
mod portal;

#[cfg(target_os = "linux")]
pub use portal::PortalHotkey;

#[cfg(not(target_os = "linux"))]
mod stub;

#[cfg(not(target_os = "linux"))]
pub use stub::PortalHotkey;

/// Outcome of attempting to register a global shortcut via the desktop portal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyRegistration {
    /// Successfully registered; the portal will deliver activations.
    Registered,
    /// Portal is not available on this desktop environment.
    Unavailable,
}

/// Trait for hotkey detection backends.
pub trait HotkeyBackend {
    /// Attempt to register the "clean clipboard" global shortcut.
    fn register(&mut self) -> Result<HotkeyRegistration, HotkeyError>;

    /// Block until the next hotkey activation, returning when the user triggers
    /// the shortcut. Returns Err if the portal session was closed.
    fn wait_for_activation(&mut self) -> Result<(), HotkeyError>;
}

/// Errors from hotkey registration and activation.
#[derive(Debug, thiserror::Error)]
pub enum HotkeyError {
    #[error("portal not available")]
    Unavailable,
    #[error("portal session closed")]
    SessionClosed,
    #[error("portal error: {0}")]
    Portal(String),
}
