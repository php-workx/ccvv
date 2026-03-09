use crate::backend::{BackendError, ClipboardBackend, ClipboardSnapshot, WriteToken};
use crate::ui_protocol::BackendCapability;

#[derive(Clone, Debug, Default)]
pub struct NoneBackend {
    last_written_text: Option<String>,
}

impl NoneBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn last_written_text(&self) -> Option<&str> {
        self.last_written_text.as_deref()
    }
}

impl ClipboardBackend for NoneBackend {
    fn capability(&self) -> BackendCapability {
        BackendCapability::DiagnosticsOnly
    }

    fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, BackendError> {
        Err(BackendError::Unavailable)
    }

    fn write_plain_text(&mut self, text: &str) -> Result<WriteToken, BackendError> {
        self.last_written_text = Some(text.to_string());
        Ok(WriteToken {
            backend_serial: None,
        })
    }

    fn source_name(&self) -> &'static str {
        match self.last_written_text {
            Some(_) => "none-write",
            None => "none",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::backend::none::NoneBackend;
    use crate::backend::ClipboardBackend;
    use crate::ui_protocol::BackendCapability;

    #[test]
    fn test_none_backend_reports_diagnostics_only() {
        let backend = NoneBackend::new();

        assert_eq!(backend.capability(), BackendCapability::DiagnosticsOnly);
    }

    #[test]
    fn test_none_backend_accepts_plain_text_writes() {
        let mut backend = NoneBackend::new();
        let token = backend.write_plain_text("cleaned text").unwrap();

        assert_eq!(token.backend_serial, None);
        assert_eq!(backend.last_written_text(), Some("cleaned text"));
    }
}
