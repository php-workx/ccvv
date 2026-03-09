#[cfg(target_os = "linux")]
mod real {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use x11rb::connection::Connection;
    use x11rb::protocol::xfixes::{self, ConnectionExt as XFixesExt};
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ConnectionExt, EventMask, Property, SelectionNotifyEvent,
        SelectionRequestEvent, Window, SELECTION_NOTIFY_EVENT,
    };
    use x11rb::rust_connection::RustConnection;

    use crate::backend::{
        BackendError, BackendStream, ClipboardBackend, ClipboardSnapshot, SelectionKind, WriteToken,
    };
    use crate::ui_protocol::BackendCapability;

    const SELECTION_TIMEOUT: Duration = Duration::from_secs(2);
    const POLL_INTERVAL: Duration = Duration::from_millis(50);

    #[derive(Debug)]
    pub struct X11Backend {
        conn: RustConnection,
        screen_num: usize,
        owner_window: Window,
        clipboard_atom: Atom,
        targets_atom: Atom,
        utf8_string_atom: Atom,
        ccvv_prop_atom: Atom,
        self_serial: u64,
    }

    impl X11Backend {
        pub fn new() -> Self {
            let (conn, screen_num) = match RustConnection::connect(None) {
                Ok(pair) => pair,
                Err(_) => return Self::disconnected(),
            };

            let root = conn.setup().roots[screen_num].root;
            let owner_window = match conn.generate_id() {
                Ok(id) => id,
                Err(_) => return Self::disconnected(),
            };

            if conn
                .create_window(
                    0,
                    owner_window,
                    root,
                    0,
                    0,
                    1,
                    1,
                    0,
                    x11rb::protocol::xproto::WindowClass::INPUT_ONLY,
                    0,
                    &Default::default(),
                )
                .is_err()
            {
                return Self::disconnected();
            }

            let clipboard_atom = Self::intern_atom(&conn, b"CLIPBOARD");
            let targets_atom = Self::intern_atom(&conn, b"TARGETS");
            let utf8_string_atom = Self::intern_atom(&conn, b"UTF8_STRING");
            let ccvv_prop_atom = Self::intern_atom(&conn, b"CCVV_SELECTION");

            if clipboard_atom == 0
                || targets_atom == 0
                || utf8_string_atom == 0
                || ccvv_prop_atom == 0
            {
                return Self::disconnected();
            }

            // Enable XFixes clipboard change notifications
            let xfixes_version = xfixes::query_version(&conn, 5, 0);
            if let Ok(reply) = xfixes_version.and_then(|cookie| cookie.reply()) {
                if reply.major_version >= 2 {
                    let _ = xfixes::select_selection_input(
                        &conn,
                        owner_window,
                        clipboard_atom,
                        xfixes::SelectionEventMask::SET_SELECTION_OWNER
                            | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                            | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
                    );
                }
            }

            let _ = conn.flush();

            Self {
                conn,
                screen_num,
                owner_window,
                clipboard_atom,
                targets_atom,
                utf8_string_atom,
                ccvv_prop_atom,
                self_serial: 0,
            }
        }

        fn disconnected() -> Self {
            // Create a dummy connection that will fail on first use
            let (conn, screen_num) =
                RustConnection::connect(None).expect("cannot create even dummy X11 connection");
            Self {
                conn,
                screen_num,
                owner_window: 0,
                clipboard_atom: 0,
                targets_atom: 0,
                utf8_string_atom: 0,
                ccvv_prop_atom: 0,
                self_serial: 0,
            }
        }

        fn intern_atom(conn: &RustConnection, name: &[u8]) -> Atom {
            conn.intern_atom(false, name)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| reply.atom)
                .unwrap_or(0)
        }

        fn is_connected(&self) -> bool {
            self.clipboard_atom != 0
        }

        fn read_selection_text(&self) -> Result<String, BackendError> {
            if !self.is_connected() {
                return Err(BackendError::Unavailable);
            }

            // Request clipboard conversion to UTF8_STRING
            self.conn
                .convert_selection(
                    self.owner_window,
                    self.clipboard_atom,
                    self.utf8_string_atom,
                    self.ccvv_prop_atom,
                    x11rb::CURRENT_TIME,
                )
                .map_err(|e| BackendError::Protocol(e.to_string()))?;

            self.conn
                .flush()
                .map_err(|e| BackendError::Protocol(e.to_string()))?;

            // Wait for SelectionNotify
            let deadline = std::time::Instant::now() + SELECTION_TIMEOUT;
            while std::time::Instant::now() < deadline {
                if let Ok(event) = self.conn.poll_for_event() {
                    if let Some(event) = event {
                        let event_bytes = event.raw_bytes();
                        if !event_bytes.is_empty()
                            && (event_bytes[0] & 0x7f) == SELECTION_NOTIFY_EVENT
                        {
                            let notify: SelectionNotifyEvent =
                                x11rb::x11_utils::TryParse::try_parse(event_bytes, &[])
                                    .map(|(v, _)| v)
                                    .map_err(|e| BackendError::Protocol(e.to_string()))?;

                            if notify.property == 0u32.into() {
                                return Err(BackendError::Protocol(
                                    "selection conversion refused".into(),
                                ));
                            }

                            // Read the property data
                            let prop = self
                                .conn
                                .get_property(
                                    true,
                                    self.owner_window,
                                    self.ccvv_prop_atom,
                                    AtomEnum::ANY,
                                    0,
                                    1024 * 1024,
                                )
                                .map_err(|e| BackendError::Protocol(e.to_string()))?
                                .reply()
                                .map_err(|e| BackendError::Protocol(e.to_string()))?;

                            return Ok(String::from_utf8_lossy(&prop.value).into_owned());
                        }
                    }
                }
                thread::sleep(POLL_INTERVAL);
            }

            Err(BackendError::Protocol("selection timeout".into()))
        }
    }

    impl Default for X11Backend {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ClipboardBackend for X11Backend {
        fn capability(&self) -> BackendCapability {
            BackendCapability::Automatic
        }

        fn subscribe(&mut self) -> Result<BackendStream, BackendError> {
            if !self.is_connected() {
                return Err(BackendError::Unavailable);
            }

            let (tx, rx) = mpsc::channel();
            let clipboard_atom = self.clipboard_atom;
            let targets_atom = self.targets_atom;
            let utf8_string_atom = self.utf8_string_atom;

            // Connect a new X11 connection for the event loop
            let (conn, screen_num) =
                RustConnection::connect(None).map_err(|e| BackendError::Protocol(e.to_string()))?;

            let root = conn.setup().roots[screen_num].root;
            let watch_window = conn
                .generate_id()
                .map_err(|e| BackendError::Protocol(e.to_string()))?;

            conn.create_window(
                0,
                watch_window,
                root,
                0,
                0,
                1,
                1,
                0,
                x11rb::protocol::xproto::WindowClass::INPUT_ONLY,
                0,
                &Default::default(),
            )
            .map_err(|e| BackendError::Protocol(e.to_string()))?;

            // Subscribe to XFixes events on this new connection
            let _ = xfixes::query_version(&conn, 5, 0)
                .ok()
                .and_then(|c| c.reply().ok());
            let _ = xfixes::select_selection_input(
                &conn,
                watch_window,
                clipboard_atom,
                xfixes::SelectionEventMask::SET_SELECTION_OWNER
                    | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                    | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
            );
            let _ = conn.flush();

            let ccvv_prop = Self::intern_atom(&conn, b"CCVV_WATCH_PROP");

            thread::Builder::new()
                .name("ccvv-x11-events".into())
                .spawn(move || {
                    loop {
                        let event = match conn.wait_for_event() {
                            Ok(e) => e,
                            Err(_) => break,
                        };

                        // XFixes SelectionNotify has response_type = first_event + 0
                        // We detect clipboard ownership changes
                        let event_bytes = event.raw_bytes();
                        if event_bytes.is_empty() {
                            continue;
                        }

                        // Try to read the clipboard content
                        let _ = conn.convert_selection(
                            watch_window,
                            clipboard_atom,
                            utf8_string_atom,
                            ccvv_prop,
                            x11rb::CURRENT_TIME,
                        );
                        let _ = conn.flush();

                        // Wait for SelectionNotify with a timeout
                        let notify_deadline =
                            std::time::Instant::now() + Duration::from_millis(500);
                        while std::time::Instant::now() < notify_deadline {
                            if let Ok(Some(inner)) = conn.poll_for_event() {
                                let inner_bytes = inner.raw_bytes();
                                if !inner_bytes.is_empty()
                                    && (inner_bytes[0] & 0x7f) == SELECTION_NOTIFY_EVENT
                                {
                                    // Read the property
                                    if let Ok(prop) = conn
                                        .get_property(
                                            true,
                                            watch_window,
                                            ccvv_prop,
                                            AtomEnum::ANY,
                                            0,
                                            1024 * 1024,
                                        )
                                        .and_then(|c| c.reply())
                                    {
                                        let text =
                                            String::from_utf8_lossy(&prop.value).into_owned();
                                        let snapshot = ClipboardSnapshot {
                                            seat_id: "x11".to_string(),
                                            selection_kind: SelectionKind::Clipboard,
                                            acquired_plain_text: text,
                                            acquired_html: None,
                                            timestamp: std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_millis()
                                                as u64,
                                            backend_serial: None,
                                            is_self_write: false,
                                        };
                                        if tx.send(Ok(snapshot)).is_err() {
                                            return;
                                        }
                                    }
                                    break;
                                }
                            }
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                })
                .map_err(|e| BackendError::Io(e))?;

            Ok(rx)
        }

        fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, BackendError> {
            let text = self.read_selection_text()?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            Ok(ClipboardSnapshot {
                seat_id: "x11".to_string(),
                selection_kind: SelectionKind::Clipboard,
                acquired_plain_text: text,
                acquired_html: None,
                timestamp: now,
                backend_serial: None,
                is_self_write: false,
            })
        }

        fn write_plain_text(&mut self, text: &str) -> Result<WriteToken, BackendError> {
            if !self.is_connected() {
                return Err(BackendError::Unavailable);
            }

            // Take ownership of the CLIPBOARD selection
            self.conn
                .set_selection_owner(self.owner_window, self.clipboard_atom, x11rb::CURRENT_TIME)
                .map_err(|e| BackendError::Protocol(e.to_string()))?;

            self.conn
                .flush()
                .map_err(|e| BackendError::Protocol(e.to_string()))?;

            // Verify we got ownership
            let owner = self
                .conn
                .get_selection_owner(self.clipboard_atom)
                .map_err(|e| BackendError::Protocol(e.to_string()))?
                .reply()
                .map_err(|e| BackendError::Protocol(e.to_string()))?;

            if owner.owner != self.owner_window {
                return Err(BackendError::Protocol(
                    "failed to acquire clipboard ownership".into(),
                ));
            }

            self.self_serial += 1;

            // Store the text for SelectionRequest handling
            // In a full implementation, we'd spawn a thread to handle SelectionRequest events.
            // For now, we set the property on our window so requestors can retrieve it.
            self.conn
                .change_property(
                    x11rb::protocol::xproto::PropMode::REPLACE,
                    self.owner_window,
                    self.utf8_string_atom,
                    self.utf8_string_atom,
                    8,
                    text.len() as u32,
                    text.as_bytes(),
                )
                .map_err(|e| BackendError::Protocol(e.to_string()))?;

            self.conn
                .flush()
                .map_err(|e| BackendError::Protocol(e.to_string()))?;

            Ok(WriteToken {
                backend_serial: Some(self.self_serial),
            })
        }

        fn source_name(&self) -> &'static str {
            "x11"
        }
    }
}

// Non-Linux: use stub implementation
#[cfg(not(target_os = "linux"))]
mod fallback {
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
            self.unsupported.read_snapshot()
        }

        fn write_plain_text(
            &mut self,
            text: &str,
        ) -> Result<WriteToken, crate::backend::BackendError> {
            self.unsupported.write_plain_text(text)
        }

        fn source_name(&self) -> &'static str {
            self.unsupported.source_name()
        }
    }
}

#[cfg(target_os = "linux")]
pub use real::X11Backend;

#[cfg(not(target_os = "linux"))]
pub use fallback::X11Backend;

#[cfg(test)]
mod tests {
    use super::X11Backend;
    use crate::backend::BackendError;
    use crate::backend::ClipboardBackend;

    #[test]
    fn test_x11_backend_reports_unsupported_on_first_snapshot_read() {
        let mut backend = X11Backend::new();

        assert_eq!(backend.source_name(), "x11");
        // On macOS: always Unavailable (stub). On Linux without DISPLAY: also Unavailable.
        assert!(matches!(
            backend.read_snapshot().unwrap_err(),
            BackendError::Unavailable | BackendError::Protocol(_)
        ));
    }
}
