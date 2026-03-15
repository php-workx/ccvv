#[cfg(all(target_os = "linux", feature = "x11"))]
mod real {
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use x11rb::connection::Connection;
    use x11rb::protocol::xfixes::{self, SelectionNotifyEvent as XfixesSelectionNotifyEvent};
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ConnectionExt, EventMask, Property, SelectionNotifyEvent,
        SelectionRequestEvent, Window, SELECTION_NOTIFY_EVENT,
    };
    use x11rb::protocol::Event;
    use x11rb::rust_connection::RustConnection;

    use crate::backend::{
        BackendError, BackendStream, ClipboardBackend, ClipboardSnapshot, SelectionKind, WriteToken,
    };
    use crate::ui_protocol::BackendCapability;

    const SELECTION_TIMEOUT: Duration = Duration::from_secs(2);
    const POLL_INTERVAL: Duration = Duration::from_millis(50);
    const INCR_THRESHOLD: usize = 256 * 1024; // 256 KiB
    const INCR_CHUNK_SIZE: usize = 64 * 1024; // 64 KiB per INCR chunk
    const INCR_MAX_SIZE: usize = 16 * 1024 * 1024; // 16 MiB cap

    #[derive(Debug)]
    pub struct X11Backend {
        conn: RustConnection,
        owner_window: Window,
        clipboard_atom: Atom,
        utf8_string_atom: Atom,
        ccvv_prop_atom: Atom,
        incr_atom: Atom,
        self_serial: u64,
        self_write_text: Arc<Mutex<Option<String>>>,
        owner: Option<X11SelectionOwnerHandle>,
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
            let utf8_string_atom = Self::intern_atom(&conn, b"UTF8_STRING");
            let ccvv_prop_atom = Self::intern_atom(&conn, b"CCVV_SELECTION");
            let incr_atom = Self::intern_atom(&conn, b"INCR");

            if clipboard_atom == 0 || utf8_string_atom == 0 || ccvv_prop_atom == 0 {
                return Self::disconnected();
            }

            // Enable XFixes clipboard change notifications
            if let Some(reply) = xfixes::query_version(&conn, 5, 0)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
            {
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
                owner_window,
                clipboard_atom,
                utf8_string_atom,
                ccvv_prop_atom,
                incr_atom,
                self_serial: 0,
                self_write_text: Arc::new(Mutex::new(None)),
                owner: None,
            }
        }

        fn disconnected() -> Self {
            // Create a dummy connection that will fail on first use
            let (conn, _screen_num) =
                RustConnection::connect(None).expect("cannot create even dummy X11 connection");
            Self {
                conn,
                owner_window: 0,
                clipboard_atom: 0,
                utf8_string_atom: 0,
                ccvv_prop_atom: 0,
                incr_atom: 0,
                self_serial: 0,
                self_write_text: Arc::new(Mutex::new(None)),
                owner: None,
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
                        if let Event::SelectionNotify(notify) = event {
                            if u32::from(notify.property) == 0 {
                                return Err(BackendError::Protocol(
                                    "selection conversion refused".into(),
                                ));
                            }

                            // Read the property data (may be INCR)
                            let prop = self
                                .conn
                                .get_property(
                                    false, // don't delete yet
                                    self.owner_window,
                                    self.ccvv_prop_atom,
                                    AtomEnum::ANY,
                                    0,
                                    1024 * 1024,
                                )
                                .map_err(|e| BackendError::Protocol(e.to_string()))?
                                .reply()
                                .map_err(|e| BackendError::Protocol(e.to_string()))?;

                            // Check if this is an INCR transfer
                            if self.incr_atom != 0 && prop.type_ == self.incr_atom {
                                // INCR: delete property to signal readiness, then
                                // accumulate chunks via PropertyNotify
                                let _ = self
                                    .conn
                                    .delete_property(self.owner_window, self.ccvv_prop_atom);
                                let _ = self.conn.change_window_attributes(
                                    self.owner_window,
                                    &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                                        .event_mask(EventMask::PROPERTY_CHANGE),
                                );
                                let _ = self.conn.flush();
                                return self.receive_incr_chunks();
                            }

                            // Normal (non-INCR): delete property and return
                            let _ = self
                                .conn
                                .delete_property(self.owner_window, self.ccvv_prop_atom);
                            return Ok(String::from_utf8_lossy(&prop.value).into_owned());
                        }
                    }
                }
                thread::sleep(POLL_INTERVAL);
            }

            Err(BackendError::Protocol("selection timeout".into()))
        }

        /// Receive INCR transfer chunks by watching PropertyNotify events.
        fn receive_incr_chunks(&self) -> Result<String, BackendError> {
            let mut buffer = Vec::new();
            let incr_deadline = std::time::Instant::now() + Duration::from_secs(10);

            while std::time::Instant::now() < incr_deadline {
                if let Ok(Some(event)) = self.conn.poll_for_event() {
                    if let Event::PropertyNotify(_) = event {
                        let prop = self
                            .conn
                            .get_property(
                                true, // delete after reading
                                self.owner_window,
                                self.ccvv_prop_atom,
                                AtomEnum::ANY,
                                0,
                                1024 * 1024,
                            )
                            .map_err(|e| BackendError::Protocol(e.to_string()))?
                            .reply()
                            .map_err(|e| BackendError::Protocol(e.to_string()))?;

                        if prop.value.is_empty() {
                            // Empty property = INCR transfer complete
                            break;
                        }

                        buffer.extend_from_slice(&prop.value);

                        if buffer.len() > INCR_MAX_SIZE {
                            return Err(BackendError::Protocol(format!(
                                "INCR transfer exceeds {} MiB cap",
                                INCR_MAX_SIZE / (1024 * 1024)
                            )));
                        }
                    }
                } else {
                    thread::sleep(POLL_INTERVAL);
                }
            }

            // Disable PropertyNotify after INCR completes
            let _ = self.conn.change_window_attributes(
                self.owner_window,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::NO_EVENT),
            );
            let _ = self.conn.flush();

            Ok(String::from_utf8_lossy(&buffer).into_owned())
        }

        fn ensure_selection_owner(&mut self) -> Result<&X11SelectionOwnerHandle, BackendError> {
            if self.owner.is_none() {
                self.owner = Some(X11SelectionOwnerHandle::start()?);
            }

            Ok(self
                .owner
                .as_ref()
                .expect("x11 selection owner initialized"))
        }
    }

    impl Default for X11Backend {
        fn default() -> Self {
            Self::new()
        }
    }

    #[derive(Debug)]
    struct X11SelectionOwnerHandle {
        commands: mpsc::Sender<OwnerCommand>,
    }

    #[derive(Debug)]
    enum OwnerCommand {
        Offer {
            text: String,
            response: mpsc::Sender<Result<(), BackendError>>,
        },
        Shutdown,
    }

    #[derive(Debug)]
    struct X11SelectionOwner {
        conn: RustConnection,
        owner_window: Window,
        clipboard_atom: Atom,
        targets_atom: Atom,
        utf8_string_atom: Atom,
        incr_atom: Atom,
        clipboard_manager_atom: Atom,
        save_targets_atom: Atom,
        has_clipboard_manager: bool,
        pending_write: Option<Vec<u8>>,
    }

    impl X11SelectionOwnerHandle {
        fn start() -> Result<Self, BackendError> {
            let (commands, receiver) = mpsc::channel();
            thread::Builder::new()
                .name("ccvv-x11-owner".into())
                .spawn(move || {
                    let mut owner = match X11SelectionOwner::new() {
                        Ok(owner) => owner,
                        Err(error) => {
                            while let Ok(command) = receiver.recv() {
                                match command {
                                    OwnerCommand::Offer { response, .. } => {
                                        let _ = response.send(Err(BackendError::Protocol(
                                            format!("failed to start X11 selection owner: {error}"),
                                        )));
                                    }
                                    OwnerCommand::Shutdown => break,
                                }
                            }
                            return;
                        }
                    };

                    owner.run(receiver);
                })
                .map_err(BackendError::Io)?;

            Ok(Self { commands })
        }

        fn offer_text(&self, text: String) -> Result<(), BackendError> {
            let (response_tx, response_rx) = mpsc::channel();
            self.commands
                .send(OwnerCommand::Offer {
                    text,
                    response: response_tx,
                })
                .map_err(|_| BackendError::SessionEnded)?;
            response_rx.recv().map_err(|_| BackendError::SessionEnded)?
        }
    }

    impl Drop for X11SelectionOwnerHandle {
        fn drop(&mut self) {
            let _ = self.commands.send(OwnerCommand::Shutdown);
        }
    }

    impl X11SelectionOwner {
        fn new() -> Result<Self, BackendError> {
            let (conn, screen_num) =
                RustConnection::connect(None).map_err(|e| BackendError::Protocol(e.to_string()))?;
            let root = conn.setup().roots[screen_num].root;
            let owner_window = conn
                .generate_id()
                .map_err(|e| BackendError::Protocol(e.to_string()))?;

            conn.create_window(
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
            .map_err(|e| BackendError::Protocol(e.to_string()))?;

            let clipboard_atom = X11Backend::intern_atom(&conn, b"CLIPBOARD");
            let targets_atom = X11Backend::intern_atom(&conn, b"TARGETS");
            let utf8_string_atom = X11Backend::intern_atom(&conn, b"UTF8_STRING");
            let incr_atom = X11Backend::intern_atom(&conn, b"INCR");
            let clipboard_manager_atom = X11Backend::intern_atom(&conn, b"CLIPBOARD_MANAGER");
            let save_targets_atom = X11Backend::intern_atom(&conn, b"SAVE_TARGETS");

            let has_clipboard_manager = if clipboard_manager_atom != 0 {
                conn.get_selection_owner(clipboard_manager_atom)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .map(|reply| reply.owner != 0)
                    .unwrap_or(false)
            } else {
                false
            };

            if has_clipboard_manager {
                eprintln!("ccvv-linux: clipboard manager detected, durability enhanced");
            } else {
                eprintln!("ccvv-linux: no clipboard manager detected");
            }

            Ok(Self {
                conn,
                owner_window,
                clipboard_atom,
                targets_atom,
                utf8_string_atom,
                incr_atom,
                clipboard_manager_atom,
                save_targets_atom,
                has_clipboard_manager,
                pending_write: None,
            })
        }

        fn run(&mut self, receiver: mpsc::Receiver<OwnerCommand>) {
            loop {
                while let Ok(command) = receiver.try_recv() {
                    match command {
                        OwnerCommand::Offer { text, response } => {
                            let _ = response.send(self.offer_text(text));
                        }
                        OwnerCommand::Shutdown => return,
                    }
                }

                match self.conn.poll_for_event() {
                    Ok(Some(event)) => match event {
                        Event::SelectionRequest(req) => self.handle_selection_request(&req),
                        Event::SelectionClear(_) => {
                            self.pending_write = None;
                        }
                        _ => {}
                    },
                    Ok(None) => thread::sleep(POLL_INTERVAL),
                    Err(_) => return,
                }
            }
        }

        fn offer_text(&mut self, text: String) -> Result<(), BackendError> {
            self.pending_write = Some(text.into_bytes());
            self.conn
                .set_selection_owner(self.owner_window, self.clipboard_atom, x11rb::CURRENT_TIME)
                .map_err(|e| BackendError::Protocol(e.to_string()))?;
            self.conn
                .flush()
                .map_err(|e| BackendError::Protocol(e.to_string()))?;

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

            if self.has_clipboard_manager && self.save_targets_atom != 0 {
                let _ = self.conn.convert_selection(
                    self.owner_window,
                    self.clipboard_manager_atom,
                    self.save_targets_atom,
                    self.targets_atom,
                    x11rb::CURRENT_TIME,
                );
                let _ = self.conn.flush();
            }

            Ok(())
        }

        fn handle_selection_request(&self, req: &SelectionRequestEvent) {
            let data = match &self.pending_write {
                Some(d) => d,
                None => {
                    self.send_selection_notify(req, 0u32.into());
                    return;
                }
            };

            // If requestor did not specify a property, use the target atom
            let property = if u32::from(req.property) == 0 {
                req.target
            } else {
                req.property
            };

            if req.target == self.targets_atom {
                // Respond with our supported TARGETS list
                let targets_raw: Vec<u8> = [self.targets_atom, self.utf8_string_atom]
                    .iter()
                    .flat_map(|a| a.to_ne_bytes())
                    .collect();
                let _ = self.conn.change_property(
                    x11rb::protocol::xproto::PropMode::REPLACE,
                    req.requestor,
                    property,
                    AtomEnum::ATOM,
                    32,
                    2,
                    &targets_raw,
                );
                self.send_selection_notify(req, property);
            } else if req.target == self.utf8_string_atom {
                if data.len() > INCR_THRESHOLD {
                    self.send_incr_to_requestor(req, property, data);
                } else {
                    let _ = self.conn.change_property(
                        x11rb::protocol::xproto::PropMode::REPLACE,
                        req.requestor,
                        property,
                        self.utf8_string_atom,
                        8,
                        data.len() as u32,
                        data,
                    );
                    self.send_selection_notify(req, property);
                }
            } else {
                // Refuse — unsupported target
                self.send_selection_notify(req, 0u32.into());
            }
        }

        /// INCR send: stream large payloads in chunks to the requestor.
        fn send_incr_to_requestor(&self, req: &SelectionRequestEvent, property: Atom, data: &[u8]) {
            // Subscribe to PropertyNotify on requestor's window
            let _ = self.conn.change_window_attributes(
                req.requestor,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::PROPERTY_CHANGE),
            );

            // Set INCR type with total size as the initial property value
            let size_bytes = (data.len() as u32).to_ne_bytes();
            let _ = self.conn.change_property(
                x11rb::protocol::xproto::PropMode::REPLACE,
                req.requestor,
                property,
                self.incr_atom,
                32,
                1,
                &size_bytes,
            );
            self.send_selection_notify(req, property);
            let _ = self.conn.flush();

            // Send chunks as requestor deletes property
            let mut offset = 0;
            let deadline = std::time::Instant::now() + Duration::from_secs(10);

            while offset < data.len() && std::time::Instant::now() < deadline {
                match self.conn.poll_for_event() {
                    Ok(Some(event)) => {
                        if let Event::PropertyNotify(notify) = event {
                            if notify.state == Property::DELETE {
                                let chunk_end = (offset + INCR_CHUNK_SIZE).min(data.len());
                                let chunk = &data[offset..chunk_end];
                                let _ = self.conn.change_property(
                                    x11rb::protocol::xproto::PropMode::REPLACE,
                                    req.requestor,
                                    property,
                                    self.utf8_string_atom,
                                    8,
                                    chunk.len() as u32,
                                    chunk,
                                );
                                let _ = self.conn.flush();
                                offset = chunk_end;
                            }
                        }
                    }
                    Ok(None) => thread::sleep(POLL_INTERVAL),
                    Err(_) => break,
                }
            }

            // Wait for final delete then send empty property to signal completion
            let empty_deadline = std::time::Instant::now() + Duration::from_secs(2);
            while std::time::Instant::now() < empty_deadline {
                match self.conn.poll_for_event() {
                    Ok(Some(event)) => {
                        if let Event::PropertyNotify(notify) = event {
                            if notify.state == Property::DELETE {
                                let _ = self.conn.change_property(
                                    x11rb::protocol::xproto::PropMode::REPLACE,
                                    req.requestor,
                                    property,
                                    self.utf8_string_atom,
                                    8,
                                    0,
                                    &[],
                                );
                                let _ = self.conn.flush();
                                break;
                            }
                        }
                    }
                    Ok(None) => thread::sleep(POLL_INTERVAL),
                    Err(_) => break,
                }
            }

            // Unsubscribe from requestor's PropertyNotify
            let _ = self.conn.change_window_attributes(
                req.requestor,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::NO_EVENT),
            );
            let _ = self.conn.flush();
        }

        fn send_selection_notify(&self, req: &SelectionRequestEvent, property: Atom) {
            let event = SelectionNotifyEvent {
                response_type: SELECTION_NOTIFY_EVENT,
                sequence: 0,
                time: req.time,
                requestor: req.requestor,
                selection: req.selection,
                target: req.target,
                property,
            };
            let _ = self
                .conn
                .send_event(false, req.requestor, EventMask::NO_EVENT, event);
            let _ = self.conn.flush();
        }
    }

    pub(super) fn take_self_write_flag(
        self_write_text: &Arc<Mutex<Option<String>>>,
        text: &str,
    ) -> bool {
        let mut pending = self_write_text
            .lock()
            .expect("x11 self-write state poisoned");
        match pending.as_ref() {
            Some(pending_text) if pending_text == text => {
                pending.take();
                true
            }
            _ => false,
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
            let utf8_string_atom = self.utf8_string_atom;
            let self_write_text = self.self_write_text.clone();

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

                        if !matches!(
                            event,
                            Event::XfixesSelectionNotify(XfixesSelectionNotifyEvent { .. })
                        ) {
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
                                if matches!(
                                    inner,
                                    Event::SelectionNotify(SelectionNotifyEvent { .. })
                                ) {
                                    // Read the property
                                    if let Some(prop) = conn
                                        .get_property(
                                            true,
                                            watch_window,
                                            ccvv_prop,
                                            AtomEnum::ANY,
                                            0,
                                            1024 * 1024,
                                        )
                                        .ok()
                                        .and_then(|c| c.reply().ok())
                                    {
                                        let text =
                                            String::from_utf8_lossy(&prop.value).into_owned();
                                        let is_self_write =
                                            take_self_write_flag(&self_write_text, &text);
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
                                            is_self_write,
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
            let is_self_write = take_self_write_flag(&self.self_write_text, &text);
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
                is_self_write,
            })
        }

        fn write_plain_text(&mut self, text: &str) -> Result<WriteToken, BackendError> {
            if !self.is_connected() {
                return Err(BackendError::Unavailable);
            }

            self.self_serial += 1;
            self.ensure_selection_owner()?
                .offer_text(text.to_string())?;
            *self
                .self_write_text
                .lock()
                .expect("x11 self-write state poisoned") = Some(text.to_string());

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
#[cfg(any(not(target_os = "linux"), not(feature = "x11")))]
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

#[cfg(all(target_os = "linux", feature = "x11"))]
pub use real::X11Backend;

#[cfg(any(not(target_os = "linux"), not(feature = "x11")))]
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

    #[cfg(all(target_os = "linux", feature = "x11"))]
    #[test]
    fn test_take_self_write_flag_consumes_matching_text_once() {
        use std::sync::{Arc, Mutex};

        let pending = Arc::new(Mutex::new(Some(String::from("hello"))));

        assert!(super::real::take_self_write_flag(&pending, "hello"));
        assert!(!super::real::take_self_write_flag(&pending, "hello"));
        assert!(!super::real::take_self_write_flag(&pending, "world"));
    }
}
