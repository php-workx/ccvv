use thiserror::Error;

use crate::ui_protocol::BackendCapability;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectionKind {
    Clipboard,
    Primary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardSnapshot {
    pub seat_id: String,
    pub selection_kind: SelectionKind,
    pub acquired_plain_text: String,
    pub acquired_html: Option<String>,
    pub timestamp: u64,
    pub backend_serial: Option<u64>,
    pub is_self_write: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteToken {
    pub backend_serial: Option<u64>,
}

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("backend is unavailable in the current session")]
    Unavailable,
}

pub trait ClipboardBackend {
    fn capability(&self) -> BackendCapability;
    fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, BackendError>;
    fn write_plain_text(&mut self, text: &str) -> Result<WriteToken, BackendError>;
    fn source_name(&self) -> &'static str;
}

pub mod none;
