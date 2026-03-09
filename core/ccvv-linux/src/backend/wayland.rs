#[cfg(target_os = "linux")]
mod real {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    use std::os::unix::io::AsRawFd;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use wayland_client::protocol::wl_registry;
    use wayland_client::protocol::wl_seat::WlSeat;
    use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, QueueHandle};

    use crate::backend::{
        BackendError, BackendStream, ClipboardBackend, ClipboardSnapshot, SelectionKind, WriteToken,
    };
    use crate::ui_protocol::BackendCapability;

    /// Tracks which data-control protocol version we bound.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ProtocolVersion {
        /// ext-data-control-v1 (preferred, Wayland staging)
        ExtDataControl,
        /// zwlr-data-control-unstable-v1 (fallback for Sway/wlroots)
        WlrDataControl,
    }

    #[derive(Debug)]
    pub struct WaylandBackend {
        limited: bool,
        protocol: Option<ProtocolVersion>,
        connected: bool,
        self_serial: u64,
    }

    impl WaylandBackend {
        pub fn new() -> Self {
            let (protocol, connected) = match Connection::connect_to_env() {
                Ok(conn) => {
                    let protocol = Self::negotiate_protocol(&conn);
                    (protocol, true)
                }
                Err(_) => (None, false),
            };

            if let Some(proto) = &protocol {
                let name = match proto {
                    ProtocolVersion::ExtDataControl => "ext-data-control-v1",
                    ProtocolVersion::WlrDataControl => "zwlr-data-control-unstable-v1",
                };
                eprintln!("ccvv-linux: wayland protocol negotiated: {name}");
            }

            Self {
                limited: false,
                protocol,
                connected,
                self_serial: 0,
            }
        }

        pub fn new_limited() -> Self {
            Self {
                limited: true,
                protocol: None,
                connected: false,
                self_serial: 0,
            }
        }

        fn negotiate_protocol(conn: &Connection) -> Option<ProtocolVersion> {
            // Create a minimal event queue to enumerate globals
            let display = conn.display();
            let mut event_queue: EventQueue<RegistryState> = conn.new_event_queue();
            let qh = event_queue.handle();

            let _registry = display.get_registry(&qh, ());

            let mut state = RegistryState {
                ext_data_control: false,
                wlr_data_control: false,
            };

            // Do a roundtrip to collect globals
            if event_queue.roundtrip(&mut state).is_err() {
                return None;
            }

            // Prefer ext-data-control, fall back to wlr-data-control
            if state.ext_data_control {
                Some(ProtocolVersion::ExtDataControl)
            } else if state.wlr_data_control {
                Some(ProtocolVersion::WlrDataControl)
            } else {
                None
            }
        }

        fn ensure_connected(&self) -> Result<(), BackendError> {
            if !self.connected {
                return Err(BackendError::Unavailable);
            }
            if self.protocol.is_none() && !self.limited {
                return Err(BackendError::Protocol(
                    "no data-control protocol available; \
                     compositor does not support ext-data-control-v1 or \
                     zwlr-data-control-unstable-v1"
                        .into(),
                ));
            }
            Ok(())
        }

        fn read_clipboard_via_pipe(&self) -> Result<String, BackendError> {
            self.ensure_connected()?;

            // Create a pipe pair for data transfer
            let (read_fd, write_fd) = nix_pipe().map_err(|e| BackendError::Io(e))?;

            // In a full implementation, we would:
            // 1. Create a data offer from the data-control device
            // 2. Request the "text/plain;charset=utf-8" MIME type
            // 3. The compositor writes to write_fd, we read from read_fd
            //
            // For now, since the full Wayland protocol binding requires
            // generated protocol code, we return Unavailable and let the
            // integration tests + CI verify on real Wayland.
            drop(write_fd);

            let mut file = unsafe { std::fs::File::from_raw_fd(read_fd) };
            let mut buf = String::new();
            file.read_to_string(&mut buf)
                .map_err(|e| BackendError::Io(e))?;

            if buf.is_empty() {
                return Err(BackendError::Unavailable);
            }

            Ok(buf)
        }
    }

    fn nix_pipe() -> Result<(i32, i32), std::io::Error> {
        let mut fds = [0i32; 2];
        let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
        if ret == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok((fds[0], fds[1]))
        }
    }

    impl Default for WaylandBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ClipboardBackend for WaylandBackend {
        fn capability(&self) -> BackendCapability {
            if self.limited {
                BackendCapability::Limited
            } else {
                BackendCapability::Automatic
            }
        }

        fn subscribe(&mut self) -> Result<BackendStream, BackendError> {
            self.ensure_connected()?;

            // Data-control subscription requires a persistent event loop.
            // The full implementation will:
            // 1. Bind the data-control manager
            // 2. Get a data-control device for each seat
            // 3. Listen for selection events
            // 4. On each selection event, request text/plain data via pipe
            // 5. Send ClipboardSnapshot through the channel
            Err(BackendError::Unavailable)
        }

        fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, BackendError> {
            let text = self.read_clipboard_via_pipe()?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            Ok(ClipboardSnapshot {
                seat_id: "wayland-seat0".to_string(),
                selection_kind: SelectionKind::Clipboard,
                acquired_plain_text: text,
                acquired_html: None,
                timestamp: now,
                backend_serial: None,
                is_self_write: false,
            })
        }

        fn write_plain_text(&mut self, text: &str) -> Result<WriteToken, BackendError> {
            self.ensure_connected()?;

            // The full implementation will:
            // 1. Create a data source via data-control manager
            // 2. Set MIME type "text/plain;charset=utf-8"
            // 3. Set the selection on the data-control device
            // 4. Handle the send event by writing text to the provided fd
            self.self_serial += 1;

            Err(BackendError::Unavailable)
        }

        fn source_name(&self) -> &'static str {
            if self.limited {
                "wayland-limited"
            } else {
                "wayland"
            }
        }
    }

    /// Minimal state for registry global enumeration.
    struct RegistryState {
        ext_data_control: bool,
        wlr_data_control: bool,
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for RegistryState {
        fn event(
            state: &mut Self,
            _proxy: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _data: &(),
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global {
                name: _,
                interface,
                version: _,
            } = event
            {
                match interface.as_str() {
                    "ext_data_control_manager_v1" => state.ext_data_control = true,
                    "zwlr_data_control_manager_v1" => state.wlr_data_control = true,
                    _ => {}
                }
            }
        }
    }

    // WlSeat needs a noop dispatch for the registry roundtrip
    delegate_noop!(RegistryState: ignore WlSeat);
}

// Non-Linux: use stub implementation
#[cfg(not(target_os = "linux"))]
mod fallback {
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
pub use real::WaylandBackend;

#[cfg(not(target_os = "linux"))]
pub use fallback::WaylandBackend;

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
            BackendError::Unavailable | BackendError::Protocol(_)
        ));
    }

    #[test]
    fn test_limited_wayland_backend_reports_capability() {
        let backend = WaylandBackend::new_limited();

        assert_eq!(backend.source_name(), "wayland-limited");
        assert_eq!(
            backend.capability(),
            crate::ui_protocol::BackendCapability::Limited
        );
    }
}
