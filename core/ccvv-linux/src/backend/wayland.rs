use crate::backend::stub::UnsupportedBackend;
use crate::backend::{ClipboardBackend, ClipboardSnapshot, WriteToken};
use crate::ui_protocol::{BackendCapability, BackendMode};

#[derive(Debug)]
pub struct WaylandBackend {
    unsupported: UnsupportedBackend,
}

impl WaylandBackend {
    pub fn new() -> Self {
        Self {
            unsupported: UnsupportedBackend::new(
                BackendMode::Wayland,
                BackendCapability::Automatic,
                "wayland",
            ),
        }
    }

    pub fn new_limited() -> Self {
        Self {
            unsupported: UnsupportedBackend::new(
                BackendMode::Limited,
                BackendCapability::Limited,
                "wayland-limited",
            ),
        }
    }

    fn unavailable_message() -> &'static str {
        "wayland backend is not implemented yet"
    }
}

impl Default for WaylandBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardBackend for WaylandBackend {
    fn capability(&self) -> BackendCapability {
        self.unsupported.capability()
    }

    fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, crate::backend::BackendError> {
        let _ = Self::unavailable_message();
        self.unsupported.read_snapshot()
    }

    fn write_plain_text(&mut self, text: &str) -> Result<WriteToken, crate::backend::BackendError> {
        let _ = (self.unsupported.source_name(), text.len());
        self.unsupported.write_plain_text(text)
    }

    fn source_name(&self) -> &'static str {
        self.unsupported.source_name()
    }
}

#[cfg(test)]
mod tests {
    use super::WaylandBackend;
    use crate::backend::BackendError;
    use crate::backend::ClipboardBackend;

    #[test]
    fn test_wayland_backend_reports_unsupported_on_first_snapshot_read() {
        let mut backend = WaylandBackend::new();

        assert_eq!(backend.source_name(), "wayland");
        assert!(matches!(
            backend.read_snapshot().unwrap_err(),
            BackendError::Unavailable
        ));
    }

    #[test]
    fn test_limited_wayland_backend_reports_capability() {
        let backend = WaylandBackend::new_limited();
        let _ = backend.source_name();

        assert_eq!(
            backend.capability(),
            crate::ui_protocol::BackendCapability::Limited
        );
    }
}
