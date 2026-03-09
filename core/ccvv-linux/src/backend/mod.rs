use std::sync::mpsc;
use thiserror::Error;

use crate::ui_protocol::BackendCapability;

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SelectionKind {
    Clipboard,
    Primary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct ClipboardSnapshot {
    pub seat_id: String,
    pub selection_kind: SelectionKind,
    pub acquired_plain_text: String,
    pub acquired_html: Option<String>,
    pub timestamp: u64,
    pub backend_serial: Option<u64>,
    pub is_self_write: bool,
}

impl ClipboardSnapshot {
    pub fn new(
        text: impl Into<String>,
        html: Option<String>,
        seat_id: impl Into<String>,
        serial: Option<u64>,
    ) -> Self {
        Self {
            seat_id: seat_id.into(),
            selection_kind: SelectionKind::Clipboard,
            acquired_plain_text: text.into(),
            acquired_html: html,
            timestamp: 0,
            backend_serial: serial,
            is_self_write: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct WriteToken {
    pub backend_serial: Option<u64>,
}

impl WriteToken {
    pub fn new(serial: Option<u64>) -> Self {
        Self {
            backend_serial: serial,
        }
    }
}

pub type BackendStream = mpsc::Receiver<Result<ClipboardSnapshot, BackendError>>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BackendError {
    #[error("backend is unavailable in the current session")]
    Unavailable,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("backend protocol error: {0}")]
    Protocol(String),
    #[error("session ended")]
    SessionEnded,
}

pub trait ClipboardBackend: Send {
    fn capability(&self) -> BackendCapability;
    fn subscribe(&mut self) -> Result<BackendStream, BackendError> {
        Err(BackendError::Unavailable)
    }
    fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, BackendError>;
    fn write_plain_text(&mut self, text: &str) -> Result<WriteToken, BackendError>;
    fn source_name(&self) -> &'static str;
}

pub mod none;
pub mod stub;
pub mod wayland;
pub mod x11;
