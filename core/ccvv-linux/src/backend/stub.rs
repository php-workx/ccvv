use crate::backend::{BackendError, ClipboardBackend, ClipboardSnapshot, WriteToken};
use crate::ui_protocol::{BackendCapability, BackendMode};

#[derive(Debug)]
pub struct UnsupportedBackend {
    capability: BackendCapability,
    source: &'static str,
}

impl UnsupportedBackend {
    pub const fn new(
        _backend_mode: BackendMode,
        capability: BackendCapability,
        source: &'static str,
    ) -> Self {
        Self { capability, source }
    }
}

impl ClipboardBackend for UnsupportedBackend {
    fn capability(&self) -> BackendCapability {
        self.capability
    }

    fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, BackendError> {
        Err(BackendError::Unavailable)
    }

    fn write_plain_text(&mut self, _text: &str) -> Result<WriteToken, BackendError> {
        Err(BackendError::Unavailable)
    }

    fn source_name(&self) -> &'static str {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use super::UnsupportedBackend;
    use crate::backend::{BackendError, ClipboardBackend};
    use crate::ui_protocol::{BackendCapability, BackendMode};

    #[test]
    fn test_unsupported_backend_reports_capability_and_fails_fast() {
        let mut backend = UnsupportedBackend::new(
            BackendMode::Limited,
            BackendCapability::Limited,
            "unsupported",
        );

        assert_eq!(backend.capability(), BackendCapability::Limited);
        assert_eq!(backend.source_name(), "unsupported");
        assert!(matches!(
            backend.read_snapshot().unwrap_err(),
            BackendError::Unavailable
        ));
    }
}
