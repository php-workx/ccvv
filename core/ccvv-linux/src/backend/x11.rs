use crate::backend::stub::UnsupportedBackend;
use crate::backend::{ClipboardBackend, ClipboardSnapshot, WriteToken};
use crate::ui_protocol::{BackendCapability, BackendMode};

#[derive(Debug)]
pub struct X11Backend {
    unsupported: UnsupportedBackend,
}

impl X11Backend {
    pub fn new() -> Self {
        Self {
            unsupported: UnsupportedBackend::new(
                BackendMode::X11,
                BackendCapability::Automatic,
                "x11",
            ),
        }
    }

    fn unavailable_snapshot_reason() -> &'static str {
        "x11 backend is not implemented yet"
    }
}

impl Default for X11Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipboardBackend for X11Backend {
    fn capability(&self) -> BackendCapability {
        self.unsupported.capability()
    }

    fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, crate::backend::BackendError> {
        let _ = Self::unavailable_snapshot_reason();
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
    use super::X11Backend;
    use crate::backend::BackendError;
    use crate::backend::ClipboardBackend;

    #[test]
    fn test_x11_backend_reports_unsupported_on_first_snapshot_read() {
        let mut backend = X11Backend::new();

        assert_eq!(backend.source_name(), "x11");
        assert!(matches!(
            backend.read_snapshot().unwrap_err(),
            BackendError::Unavailable
        ));
    }
}
