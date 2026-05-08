#[cfg(all(target_os = "linux", feature = "wayland"))]
mod real {
    use std::collections::{HashMap, HashSet};
    use std::io::{Read, Write};
    use std::os::fd::{AsFd, BorrowedFd};
    use std::os::unix::net::UnixStream;
    use std::process::{Command, ExitStatus, Stdio};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use wait_timeout::ChildExt;
    use wayland_client::globals::{registry_queue_init, GlobalError, GlobalListContents};
    use wayland_client::protocol::wl_registry;
    use wayland_client::protocol::wl_registry::WlRegistry;
    use wayland_client::protocol::wl_seat::WlSeat;
    use wayland_client::{
        event_created_child, Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    };
    use wayland_protocols::ext::data_control::v1::client::ext_data_control_device_v1::{
        self, ExtDataControlDeviceV1,
    };
    use wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1;
    use wayland_protocols::ext::data_control::v1::client::ext_data_control_offer_v1::{
        self, ExtDataControlOfferV1,
    };
    use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1::{
        self, ZwlrDataControlDeviceV1,
    };
    use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;
    use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_offer_v1::{
        self, ZwlrDataControlOfferV1,
    };
    use wl_clipboard_rs::copy::{
        ClipboardType as CopyClipboardType, MimeSource, MimeType as CopyMimeType,
        Options as CopyOptions, Source,
    };

    use crate::backend::{
        BackendError, BackendStream, ClipboardBackend, ClipboardSnapshot, SelectionKind, WriteToken,
    };
    use crate::ui_protocol::BackendCapability;

    pub(super) const FALLBACK_WAYLAND_SEAT: &str = "wayland-seat-0";
    const MAX_WAYLAND_TEXT_BYTES: usize = 1_048_576;
    const WAYLAND_READ_TIMEOUT: Duration = Duration::from_secs(2);

    struct CommandOutput {
        status: ExitStatus,
        stdout: Vec<u8>,
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub enum WaylandProtocol {
        ExtDataControl,
        WlrDataControl,
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub enum WaylandSupport {
        Automatic {
            protocol: WaylandProtocol,
            seat_count: usize,
        },
        NoDataControl {
            seat_count: usize,
        },
        Unavailable,
    }

    #[derive(Debug)]
    pub struct WaylandBackend {
        limited: bool,
        protocol: Option<WaylandProtocol>,
        connected: bool,
        self_serial: u64,
        last_self_text_by_seat: Arc<Mutex<HashMap<String, String>>>,
        seat_ids: Vec<String>,
        monitor: Option<WaylandMonitorHandle>,
    }

    impl WaylandBackend {
        pub fn new() -> Self {
            let mut backend = Self {
                limited: false,
                protocol: None,
                connected: false,
                self_serial: 0,
                last_self_text_by_seat: Arc::new(Mutex::new(HashMap::new())),
                seat_ids: vec![FALLBACK_WAYLAND_SEAT.to_string()],
                monitor: None,
            };

            if let Ok(conn) = Connection::connect_to_env() {
                let (protocol, seat_ids) = Self::discover_protocol(&conn);
                backend.protocol = protocol;
                backend.connected = protocol.is_some() && !seat_ids.is_empty();
                if !seat_ids.is_empty() {
                    backend.seat_ids = seat_ids;
                }

                if let Some(protocol) = protocol {
                    eprintln!(
                        "ccvv-linux: wayland protocol negotiated: {} ({} seat{})",
                        protocol.as_str(),
                        backend.seat_ids.len(),
                        if backend.seat_ids.len() == 1 { "" } else { "s" }
                    );
                    eprintln!(
                        "ccvv-linux: wayland using primary seat {}",
                        backend.primary_seat_id()
                    );
                }
            }

            backend
        }

        pub fn new_limited() -> Self {
            Self {
                limited: true,
                protocol: None,
                connected: false,
                self_serial: 0,
                last_self_text_by_seat: Arc::new(Mutex::new(HashMap::new())),
                seat_ids: vec![FALLBACK_WAYLAND_SEAT.to_string()],
                monitor: None,
            }
        }

        pub fn probe_support() -> WaylandSupport {
            if !Self::has_wayland_session_hint() {
                return WaylandSupport::Unavailable;
            }

            match Connection::connect_to_env() {
                Ok(conn) => {
                    let (protocol, seat_ids) = Self::discover_protocol(&conn);
                    match protocol {
                        Some(protocol) if !seat_ids.is_empty() => WaylandSupport::Automatic {
                            protocol,
                            seat_count: seat_ids.len(),
                        },
                        _ => WaylandSupport::NoDataControl {
                            seat_count: seat_ids.len(),
                        },
                    }
                }
                Err(_) => WaylandSupport::NoDataControl { seat_count: 0 },
            }
        }

        fn has_wayland_session_hint() -> bool {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return true;
            }

            matches!(
                std::env::var("XDG_SESSION_TYPE")
                    .unwrap_or_default()
                    .to_lowercase()
                    .as_str(),
                "wayland" | "wayland-only"
            )
        }

        fn discover_protocol(conn: &Connection) -> (Option<WaylandProtocol>, Vec<String>) {
            let display = conn.display();
            let mut event_queue: EventQueue<RegistryState> = conn.new_event_queue();
            let qh = event_queue.handle();
            let _registry = display.get_registry(&qh, ());

            let mut state = RegistryState {
                ext_data_control: false,
                wlr_data_control: false,
                seat_names: Vec::new(),
            };

            if event_queue.roundtrip(&mut state).is_err() {
                return (None, Vec::new());
            }

            let protocol = Self::select_protocol(state.ext_data_control, state.wlr_data_control);
            (protocol, state.seat_names)
        }

        pub(crate) fn select_protocol(
            ext_data_control: bool,
            wlr_data_control: bool,
        ) -> Option<WaylandProtocol> {
            if ext_data_control {
                Some(WaylandProtocol::ExtDataControl)
            } else if wlr_data_control {
                Some(WaylandProtocol::WlrDataControl)
            } else {
                None
            }
        }

        fn ensure_connected(&self) -> Result<(), BackendError> {
            if self.limited {
                if Self::limited_mode_available() {
                    return Ok(());
                }
                return Err(BackendError::Unavailable);
            }

            if !self.connected {
                return if self.protocol.is_some() {
                    Err(BackendError::Protocol(
                        "no usable wayland clipboard backend is available for automatic mode"
                            .into(),
                    ))
                } else {
                    Err(BackendError::Unavailable)
                };
            }

            Ok(())
        }

        fn ensure_monitor(&mut self) -> Result<&WaylandMonitorHandle, BackendError> {
            if self.monitor.is_none() {
                self.monitor = Some(WaylandMonitorHandle::start(
                    self.last_self_text_by_seat.clone(),
                )?);
            }

            Ok(self.monitor.as_ref().expect("wayland monitor initialized"))
        }

        fn write_clipboard_text_protocol(text: &str) -> Result<(), BackendError> {
            let mut options = CopyOptions::new();
            options.foreground(false);
            options.clipboard(CopyClipboardType::Regular);
            options
                .copy_multi(vec![MimeSource {
                    source: Source::Bytes(text.as_bytes().to_vec().into_boxed_slice()),
                    mime_type: CopyMimeType::Text,
                }])
                .map_err(|error| {
                    BackendError::Protocol(format!(
                        "failed to publish wayland clipboard contents: {error}"
                    ))
                })
        }

        pub fn limited_mode_available() -> bool {
            if let Some(forced) = Self::forced_limited_mode_result() {
                return forced;
            }

            Self::command_available("wl-paste") && Self::command_available("wl-copy")
        }

        fn forced_limited_mode_result() -> Option<bool> {
            match std::env::var("CCVV_WAYLAND_FORCE_LIMITED_TOOLS") {
                Ok(value) => match value.to_ascii_lowercase().as_str() {
                    "1" | "true" | "yes" | "available" => Some(true),
                    "0" | "false" | "no" | "unavailable" => Some(false),
                    _ => None,
                },
                Err(_) => None,
            }
        }

        fn command_available(command: &str) -> bool {
            Command::new(command)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
        }

        fn try_read_clipboard_html() -> Option<String> {
            let output = run_wl_paste_limited(
                &["--type", "text/html"],
                crate::clipboard::html::MAX_HTML_BYTES,
            )
            .ok()?;
            if !output.status.success() || output.stdout.is_empty() {
                return None;
            }
            let html = String::from_utf8_lossy(&output.stdout).into_owned();
            crate::clipboard::html::extract_plain_text_from_html(&html)
                .ok()
                .map(|_| html)
        }

        fn read_clipboard_text() -> Result<String, BackendError> {
            let output = run_wl_paste_limited(&[], MAX_WAYLAND_TEXT_BYTES)?;

            if !output.status.success() {
                return Err(BackendError::Protocol(
                    "wl-paste exited with non-zero status".into(),
                ));
            }

            String::from_utf8(output.stdout)
                .map_err(|_| BackendError::Protocol("wl-paste output was not valid UTF-8".into()))
        }

        fn write_clipboard_text(text: &str) -> Result<(), BackendError> {
            let mut child = Command::new("wl-copy")
                .stdin(Stdio::piped())
                .spawn()
                .map_err(|error| {
                    BackendError::Protocol(format!("failed to execute wl-copy: {error}"))
                })?;

            {
                let stdin = child
                    .stdin
                    .as_mut()
                    .ok_or(BackendError::Protocol("wl-copy stdin unavailable".into()))?;
                stdin
                    .write_all(text.as_bytes())
                    .map_err(|error| BackendError::Protocol(error.to_string()))?;
            }

            let status = child
                .wait()
                .map_err(|error| BackendError::Protocol(error.to_string()))?;

            if !status.success() {
                return Err(BackendError::Protocol(
                    "wl-copy exited with non-zero status".into(),
                ));
            }

            Ok(())
        }

        pub(crate) fn primary_seat_id(&self) -> &str {
            self.seat_ids
                .first()
                .map(|seat| seat.as_str())
                .unwrap_or(FALLBACK_WAYLAND_SEAT)
        }
    }

    impl Default for WaylandBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    fn read_limited_bytes<R: Read>(reader: R, max_bytes: usize) -> Result<Vec<u8>, BackendError> {
        let mut limited = reader.take((max_bytes + 1) as u64);
        let mut bytes = Vec::new();
        limited.read_to_end(&mut bytes).map_err(BackendError::Io)?;
        if bytes.len() > max_bytes {
            return Err(BackendError::Protocol(format!(
                "wayland clipboard payload exceeded {max_bytes} bytes"
            )));
        }
        Ok(bytes)
    }

    fn run_wl_paste_limited(
        args: &[&str],
        max_bytes: usize,
    ) -> Result<CommandOutput, BackendError> {
        let mut child = Command::new("wl-paste")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                BackendError::Protocol(format!("failed to execute wl-paste: {error}"))
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BackendError::Protocol("wl-paste stdout unavailable".into()))?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(read_limited_bytes(stdout, max_bytes));
        });

        let stdout = match rx.recv_timeout(WAYLAND_READ_TIMEOUT) {
            Ok(Ok(stdout)) => stdout,
            Ok(Err(error)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BackendError::Protocol(
                    "wl-paste timed out while reading clipboard data".into(),
                ));
            }
        };

        let status = match child
            .wait_timeout(WAYLAND_READ_TIMEOUT)
            .map_err(BackendError::Io)?
        {
            Some(status) => status,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BackendError::Protocol("wl-paste timed out".into()));
            }
        };

        Ok(CommandOutput { status, stdout })
    }

    impl ClipboardBackend for WaylandBackend {
        fn capability(&self) -> BackendCapability {
            if self.connected && self.protocol.is_some() && !self.limited {
                BackendCapability::Automatic
            } else {
                BackendCapability::Limited
            }
        }

        fn subscribe(&mut self) -> Result<BackendStream, BackendError> {
            self.ensure_connected()?;
            if self.limited {
                return Err(BackendError::Unavailable);
            }
            Ok(self.ensure_monitor()?.subscribe())
        }

        fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, BackendError> {
            self.ensure_connected()?;
            if self.limited {
                let text = Self::read_clipboard_text()?;
                let acquired_html = Self::try_read_clipboard_html();
                let seat_id = self.primary_seat_id().to_string();
                let is_self_write =
                    take_self_write_flag(&self.last_self_text_by_seat, &seat_id, &text);

                return Ok(ClipboardSnapshot {
                    seat_id,
                    selection_kind: SelectionKind::Clipboard,
                    acquired_plain_text: text,
                    acquired_html,
                    timestamp: current_timestamp_ms(),
                    backend_serial: Some(self.self_serial),
                    is_self_write,
                });
            }

            self.ensure_monitor()?.latest_snapshot().ok_or_else(|| {
                BackendError::Protocol(
                    "wayland clipboard has not produced an event-driven snapshot yet".into(),
                )
            })
        }

        fn write_plain_text(&mut self, text: &str) -> Result<WriteToken, BackendError> {
            self.ensure_connected()?;
            self.self_serial += 1;
            if self.limited {
                Self::write_clipboard_text(text)?;
            } else {
                Self::write_clipboard_text_protocol(text)?;
            }
            self.last_self_text_by_seat
                .lock()
                .expect("wayland self-write map poisoned")
                .insert(self.primary_seat_id().to_string(), text.to_string());
            Ok(WriteToken {
                backend_serial: Some(self.self_serial),
            })
        }

        fn source_name(&self) -> &'static str {
            if self.limited {
                "wayland-limited"
            } else {
                "wayland"
            }
        }
    }

    #[derive(Debug)]
    struct WaylandMonitorHandle {
        latest_snapshot: Arc<Mutex<Option<ClipboardSnapshot>>>,
        subscribers: Arc<Mutex<Vec<mpsc::Sender<Result<ClipboardSnapshot, BackendError>>>>>,
    }

    impl WaylandMonitorHandle {
        fn start(
            last_self_text_by_seat: Arc<Mutex<HashMap<String, String>>>,
        ) -> Result<Self, BackendError> {
            let latest_snapshot = Arc::new(Mutex::new(None));
            let subscribers = Arc::new(Mutex::new(Vec::new()));
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);

            let thread_latest_snapshot = latest_snapshot.clone();
            let thread_subscribers = subscribers.clone();

            thread::Builder::new()
                .name("ccvv-wayland-events".into())
                .spawn(move || {
                    match initialize_monitor(
                        thread_latest_snapshot.clone(),
                        thread_subscribers.clone(),
                        last_self_text_by_seat,
                    ) {
                        Ok((mut queue, mut state)) => {
                            let _ = ready_tx.send(Ok(()));
                            if let Err(error) = run_monitor_loop(&mut queue, &mut state) {
                                broadcast_error(
                                    &thread_subscribers,
                                    format!("wayland dispatch failed: {error}"),
                                );
                            }
                        }
                        Err(error) => {
                            let message = error.to_string();
                            let _ = ready_tx.send(Err(error));
                            broadcast_error(&thread_subscribers, message);
                        }
                    }
                })
                .map_err(BackendError::Io)?;

            ready_rx.recv().map_err(|_| BackendError::SessionEnded)??;

            Ok(Self {
                latest_snapshot,
                subscribers,
            })
        }

        fn latest_snapshot(&self) -> Option<ClipboardSnapshot> {
            self.latest_snapshot
                .lock()
                .expect("wayland latest snapshot poisoned")
                .clone()
        }

        fn subscribe(&self) -> BackendStream {
            let (tx, rx) = mpsc::channel();
            if let Some(snapshot) = self.latest_snapshot() {
                let _ = tx.send(Ok(snapshot));
            }
            self.subscribers
                .lock()
                .expect("wayland subscriber list poisoned")
                .push(tx);
            rx
        }
    }

    #[derive(Clone)]
    enum MonitorManager {
        Ext(ExtDataControlManagerV1),
        Wlr(ZwlrDataControlManagerV1),
    }

    impl MonitorManager {
        fn get_data_device(
            &self,
            seat: &WlSeat,
            qh: &QueueHandle<MonitorState>,
            seat_id: String,
        ) -> MonitorDevice {
            match self {
                Self::Ext(manager) => {
                    MonitorDevice::Ext(manager.get_data_device(seat, qh, seat_id))
                }
                Self::Wlr(manager) => {
                    MonitorDevice::Wlr(manager.get_data_device(seat, qh, seat_id))
                }
            }
        }
    }

    #[derive(Clone)]
    enum MonitorDevice {
        Ext(ExtDataControlDeviceV1),
        Wlr(ZwlrDataControlDeviceV1),
    }

    impl MonitorDevice {
        fn destroy(&self) {
            match self {
                Self::Ext(device) => device.destroy(),
                Self::Wlr(device) => device.destroy(),
            }
        }
    }

    #[derive(Clone, PartialEq, Eq, Hash)]
    enum MonitorOffer {
        Ext(ExtDataControlOfferV1),
        Wlr(ZwlrDataControlOfferV1),
    }

    impl MonitorOffer {
        fn destroy(&self) {
            match self {
                Self::Ext(offer) => offer.destroy(),
                Self::Wlr(offer) => offer.destroy(),
            }
        }

        fn receive(&self, mime_type: String, fd: BorrowedFd<'_>) {
            match self {
                Self::Ext(offer) => offer.receive(mime_type, fd),
                Self::Wlr(offer) => offer.receive(mime_type, fd),
            }
        }
    }

    impl From<ExtDataControlOfferV1> for MonitorOffer {
        fn from(value: ExtDataControlOfferV1) -> Self {
            Self::Ext(value)
        }
    }

    impl From<ZwlrDataControlOfferV1> for MonitorOffer {
        fn from(value: ZwlrDataControlOfferV1) -> Self {
            Self::Wlr(value)
        }
    }

    #[derive(Default)]
    struct SeatState {
        device: Option<MonitorDevice>,
        offer: Option<MonitorOffer>,
        last_text: Option<String>,
    }

    impl SeatState {
        fn set_device(&mut self, device: Option<MonitorDevice>) {
            let old_device = self.device.take();
            self.device = device;
            if let Some(device) = old_device {
                device.destroy();
            }
        }

        fn set_offer(&mut self, offer: Option<MonitorOffer>) {
            let old_offer = self.offer.take();
            self.offer = offer;
            if let Some(offer) = old_offer {
                offer.destroy();
            }
        }
    }

    struct MonitorState {
        seats_by_proxy: HashMap<WlSeat, String>,
        seats: HashMap<String, SeatState>,
        offers: HashMap<MonitorOffer, Vec<String>>,
        pending_seats: HashSet<String>,
        latest_snapshot: Arc<Mutex<Option<ClipboardSnapshot>>>,
        subscribers: Arc<Mutex<Vec<mpsc::Sender<Result<ClipboardSnapshot, BackendError>>>>>,
        last_self_text_by_seat: Arc<Mutex<HashMap<String, String>>>,
        manager: MonitorManager,
    }

    impl MonitorState {
        fn process_pending_selections(
            &mut self,
            queue: &mut EventQueue<Self>,
        ) -> Result<(), BackendError> {
            let pending = self.pending_seats.drain().collect::<Vec<_>>();
            for seat_id in pending {
                let Some(offer) = self.seats.get(&seat_id).and_then(|seat| seat.offer.clone())
                else {
                    continue;
                };

                let Some(text) = self.read_offer_text(queue, &offer)? else {
                    self.pending_seats.insert(seat_id);
                    continue;
                };

                let previous = self
                    .seats
                    .get(&seat_id)
                    .and_then(|seat| seat.last_text.as_deref());
                if previous == Some(text.as_str()) {
                    continue;
                }

                let acquired_html = self.try_read_offer_html(queue, &offer);

                let is_self_write =
                    take_self_write_flag(&self.last_self_text_by_seat, &seat_id, &text);

                if let Some(seat) = self.seats.get_mut(&seat_id) {
                    seat.last_text = Some(text.clone());
                }

                self.broadcast_snapshot(ClipboardSnapshot {
                    seat_id,
                    selection_kind: SelectionKind::Clipboard,
                    acquired_plain_text: text,
                    acquired_html,
                    timestamp: current_timestamp_ms(),
                    backend_serial: None,
                    is_self_write,
                });
            }

            Ok(())
        }

        fn try_read_offer_html(
            &mut self,
            queue: &mut EventQueue<Self>,
            offer: &MonitorOffer,
        ) -> Option<String> {
            let has_html = self
                .offers
                .get(offer)
                .map(|mimes| mimes.iter().any(|m| m == "text/html"))
                .unwrap_or(false);
            if !has_html {
                return None;
            }

            let (mut reader, writer) = UnixStream::pair().ok()?;
            offer.receive("text/html".to_string(), writer.as_fd());
            drop(writer);

            queue.roundtrip(self).ok()?;

            reader.set_read_timeout(Some(WAYLAND_READ_TIMEOUT)).ok()?;
            let bytes = read_limited_bytes(reader, crate::clipboard::html::MAX_HTML_BYTES).ok()?;
            let html = String::from_utf8(bytes).ok()?;
            crate::clipboard::html::extract_plain_text_from_html(&html)
                .ok()
                .map(|_| html)
        }

        fn read_offer_text(
            &mut self,
            queue: &mut EventQueue<Self>,
            offer: &MonitorOffer,
        ) -> Result<Option<String>, BackendError> {
            let mime_type = self
                .offers
                .get(offer)
                .and_then(|mime_types| select_text_mime(mime_types));
            let Some(mime_type) = mime_type else {
                return Ok(None);
            };

            let (mut reader, writer) = UnixStream::pair().map_err(BackendError::Io)?;
            offer.receive(mime_type, writer.as_fd());
            drop(writer);

            queue.roundtrip(self).map_err(|error| {
                BackendError::Protocol(format!(
                    "failed to receive wayland clipboard offer: {error}"
                ))
            })?;

            reader
                .set_read_timeout(Some(WAYLAND_READ_TIMEOUT))
                .map_err(BackendError::Io)?;
            let bytes = read_limited_bytes(reader, MAX_WAYLAND_TEXT_BYTES)?;
            let text = String::from_utf8(bytes).map_err(|_| {
                BackendError::Protocol("wayland clipboard offer was not valid UTF-8".into())
            })?;
            Ok(Some(text))
        }

        fn broadcast_snapshot(&mut self, snapshot: ClipboardSnapshot) {
            *self
                .latest_snapshot
                .lock()
                .expect("wayland latest snapshot poisoned") = Some(snapshot.clone());

            self.subscribers
                .lock()
                .expect("wayland subscriber list poisoned")
                .retain(|subscriber| subscriber.send(Ok(snapshot.clone())).is_ok());
        }
    }

    fn initialize_monitor(
        latest_snapshot: Arc<Mutex<Option<ClipboardSnapshot>>>,
        subscribers: Arc<Mutex<Vec<mpsc::Sender<Result<ClipboardSnapshot, BackendError>>>>>,
        last_self_text_by_seat: Arc<Mutex<HashMap<String, String>>>,
    ) -> Result<(EventQueue<MonitorState>, MonitorState), BackendError> {
        let conn = Connection::connect_to_env().map_err(|error| {
            BackendError::Protocol(format!(
                "failed to connect to wayland compositor for monitoring: {error}"
            ))
        })?;

        let (globals, mut queue) =
            registry_queue_init::<MonitorState>(&conn).map_err(|error| match error {
                GlobalError::Backend(source) => {
                    BackendError::Protocol(format!("failed to query wayland globals: {source}"))
                }
                GlobalError::InvalidId(source) => BackendError::Protocol(format!(
                    "received invalid wayland global id: {source:?}"
                )),
            })?;

        let qh = queue.handle();
        let manager = globals
            .bind(&qh, 1..=1, ())
            .ok()
            .map(MonitorManager::Ext)
            .or_else(|| globals.bind(&qh, 1..=2, ()).ok().map(MonitorManager::Wlr))
            .ok_or_else(|| {
                BackendError::Protocol(
                    "wayland compositor does not expose ext-data-control-v1 or zwlr-data-control"
                        .into(),
                )
            })?;

        let registry = globals.registry();
        let seats_by_proxy = globals.contents().with_list(|list| {
            list.iter()
                .filter(|global| {
                    global.interface == WlSeat::interface().name && global.version >= 2
                })
                .map(|global| {
                    let seat_id = format!("wayland-seat-{}", global.name);
                    let seat = registry.bind(global.name, 2, &qh, ());
                    (seat, seat_id)
                })
                .collect::<HashMap<_, _>>()
        });

        if seats_by_proxy.is_empty() {
            return Err(BackendError::Unavailable);
        }

        let mut seats = HashMap::new();
        for seat_id in seats_by_proxy.values() {
            seats.insert(seat_id.clone(), SeatState::default());
        }

        let mut state = MonitorState {
            seats_by_proxy,
            seats,
            offers: HashMap::new(),
            pending_seats: HashSet::new(),
            latest_snapshot,
            subscribers,
            last_self_text_by_seat,
            manager,
        };

        let seat_entries = state
            .seats_by_proxy
            .iter()
            .map(|(seat, seat_id)| (seat.clone(), seat_id.clone()))
            .collect::<Vec<_>>();
        for (seat, seat_id) in seat_entries {
            let device = state
                .manager
                .get_data_device(&seat, &queue.handle(), seat_id.clone());
            if let Some(seat_state) = state.seats.get_mut(&seat_id) {
                seat_state.set_device(Some(device));
            }
        }

        queue.roundtrip(&mut state).map_err(|error| {
            BackendError::Protocol(format!(
                "initial wayland clipboard roundtrip failed: {error}"
            ))
        })?;
        state.process_pending_selections(&mut queue)?;

        Ok((queue, state))
    }

    fn run_monitor_loop(
        queue: &mut EventQueue<MonitorState>,
        state: &mut MonitorState,
    ) -> Result<(), BackendError> {
        loop {
            queue
                .blocking_dispatch(state)
                .map_err(|error| BackendError::Protocol(format!("{error}")))?;
            state.process_pending_selections(queue)?;
        }
    }

    fn broadcast_error(
        subscribers: &Arc<Mutex<Vec<mpsc::Sender<Result<ClipboardSnapshot, BackendError>>>>>,
        message: String,
    ) {
        subscribers
            .lock()
            .expect("wayland subscriber list poisoned")
            .retain(|subscriber| {
                subscriber
                    .send(Err(BackendError::Protocol(message.clone())))
                    .is_ok()
            });
    }

    fn current_timestamp_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub(super) fn take_self_write_flag(
        last_self_text_by_seat: &Arc<Mutex<HashMap<String, String>>>,
        seat_id: &str,
        text: &str,
    ) -> bool {
        let mut by_seat = last_self_text_by_seat
            .lock()
            .expect("wayland self-write map poisoned");
        let is_self_write = matches!(by_seat.get(seat_id), Some(last) if last == text);
        if is_self_write {
            by_seat.remove(seat_id);
        }
        is_self_write
    }

    pub(super) fn select_text_mime(mime_types: &[String]) -> Option<String> {
        mime_types
            .iter()
            .find(|mime| mime.as_str() == "text/plain;charset=utf-8")
            .or_else(|| {
                mime_types
                    .iter()
                    .find(|mime| mime.as_str() == "UTF8_STRING")
            })
            .or_else(|| mime_types.iter().find(|mime| mime.starts_with("text/")))
            .or_else(|| mime_types.iter().find(|mime| mime.contains("utf8")))
            .cloned()
    }

    impl WaylandProtocol {
        fn as_str(self) -> &'static str {
            match self {
                WaylandProtocol::ExtDataControl => "ext-data-control-v1",
                WaylandProtocol::WlrDataControl => "zwlr-data-control-unstable-v1",
            }
        }

        #[cfg(test)]
        pub(crate) fn runtime_implemented(self) -> bool {
            true
        }
    }

    impl Dispatch<WlRegistry, GlobalListContents> for MonitorState {
        fn event(
            _state: &mut Self,
            _proxy: &WlRegistry,
            _event: <WlRegistry as Proxy>::Event,
            _data: &GlobalListContents,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<WlSeat, ()> for MonitorState {
        fn event(
            _state: &mut Self,
            _proxy: &WlSeat,
            _event: <WlSeat as Proxy>::Event,
            _data: &(),
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ExtDataControlManagerV1, ()> for MonitorState {
        fn event(
            _state: &mut Self,
            _proxy: &ExtDataControlManagerV1,
            _event: <ExtDataControlManagerV1 as Proxy>::Event,
            _data: &(),
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZwlrDataControlManagerV1, ()> for MonitorState {
        fn event(
            _state: &mut Self,
            _proxy: &ZwlrDataControlManagerV1,
            _event: <ZwlrDataControlManagerV1 as Proxy>::Event,
            _data: &(),
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ExtDataControlDeviceV1, String> for MonitorState {
        fn event(
            state: &mut Self,
            _proxy: &ExtDataControlDeviceV1,
            event: <ExtDataControlDeviceV1 as Proxy>::Event,
            seat_id: &String,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
            match event {
                ext_data_control_device_v1::Event::DataOffer { id } => {
                    state.offers.insert(MonitorOffer::from(id), Vec::new());
                }
                ext_data_control_device_v1::Event::Selection { id } => {
                    if let Some(seat) = state.seats.get_mut(seat_id) {
                        seat.set_offer(id.map(MonitorOffer::from));
                    }
                    state.pending_seats.insert(seat_id.clone());
                }
                ext_data_control_device_v1::Event::Finished => {
                    if let Some(seat) = state.seats.get_mut(seat_id) {
                        seat.set_device(None);
                    }
                }
                _ => {}
            }
        }

        event_created_child!(MonitorState, ExtDataControlDeviceV1, [
            ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
        ]);
    }

    impl Dispatch<ZwlrDataControlDeviceV1, String> for MonitorState {
        fn event(
            state: &mut Self,
            _proxy: &ZwlrDataControlDeviceV1,
            event: <ZwlrDataControlDeviceV1 as Proxy>::Event,
            seat_id: &String,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
            match event {
                zwlr_data_control_device_v1::Event::DataOffer { id } => {
                    state.offers.insert(MonitorOffer::from(id), Vec::new());
                }
                zwlr_data_control_device_v1::Event::Selection { id } => {
                    if let Some(seat) = state.seats.get_mut(seat_id) {
                        seat.set_offer(id.map(MonitorOffer::from));
                    }
                    state.pending_seats.insert(seat_id.clone());
                }
                zwlr_data_control_device_v1::Event::Finished => {
                    if let Some(seat) = state.seats.get_mut(seat_id) {
                        seat.set_device(None);
                    }
                }
                _ => {}
            }
        }

        event_created_child!(MonitorState, ZwlrDataControlDeviceV1, [
            zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
        ]);
    }

    impl Dispatch<ExtDataControlOfferV1, ()> for MonitorState {
        fn event(
            state: &mut Self,
            proxy: &ExtDataControlOfferV1,
            event: <ExtDataControlOfferV1 as Proxy>::Event,
            _data: &(),
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
            if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
                state
                    .offers
                    .entry(MonitorOffer::from(proxy.clone()))
                    .or_default()
                    .push(mime_type);
            }
        }
    }

    impl Dispatch<ZwlrDataControlOfferV1, ()> for MonitorState {
        fn event(
            state: &mut Self,
            proxy: &ZwlrDataControlOfferV1,
            event: <ZwlrDataControlOfferV1 as Proxy>::Event,
            _data: &(),
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
            if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
                state
                    .offers
                    .entry(MonitorOffer::from(proxy.clone()))
                    .or_default()
                    .push(mime_type);
            }
        }
    }

    struct RegistryState {
        ext_data_control: bool,
        wlr_data_control: bool,
        seat_names: Vec<String>,
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
                name: seat_name,
                interface,
                version: _,
            } = event
            {
                match interface.as_str() {
                    "ext_data_control_manager_v1" => state.ext_data_control = true,
                    "zwlr_data_control_manager_v1" => state.wlr_data_control = true,
                    "wl_seat" => state.seat_names.push(format!("wayland-seat-{seat_name}")),
                    _ => {}
                }
            }
        }
    }

    impl Dispatch<WlSeat, ()> for RegistryState {
        fn event(
            _state: &mut Self,
            _proxy: &WlSeat,
            _event: <WlSeat as Proxy>::Event,
            _data: &(),
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
        }
    }
}

#[cfg(any(not(target_os = "linux"), not(feature = "wayland")))]
mod fallback {
    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub enum WaylandProtocol {
        ExtDataControl,
        WlrDataControl,
    }

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub enum WaylandSupport {
        Automatic {
            protocol: WaylandProtocol,
            seat_count: usize,
        },
        NoDataControl {
            seat_count: usize,
        },
        Unavailable,
    }

    use crate::backend::stub::UnsupportedBackend;
    use crate::backend::{
        BackendError, BackendStream, ClipboardBackend, ClipboardSnapshot, WriteToken,
    };
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

        pub fn probe_support() -> WaylandSupport {
            if Self::has_wayland_session_hint() {
                WaylandSupport::NoDataControl { seat_count: 0 }
            } else {
                WaylandSupport::Unavailable
            }
        }

        pub fn is_protocol_available() -> bool {
            false
        }

        pub fn limited_mode_available() -> bool {
            match std::env::var("CCVV_WAYLAND_FORCE_LIMITED_TOOLS") {
                Ok(value) => matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "available"
                ),
                Err(_) => false,
            }
        }

        fn has_wayland_session_hint() -> bool {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return true;
            }

            matches!(
                std::env::var("XDG_SESSION_TYPE")
                    .unwrap_or_default()
                    .to_lowercase()
                    .as_str(),
                "wayland" | "wayland-only"
            )
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

        fn subscribe(&mut self) -> Result<BackendStream, BackendError> {
            Err(BackendError::Unavailable)
        }

        fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, BackendError> {
            self.unsupported.read_snapshot()
        }

        fn write_plain_text(&mut self, text: &str) -> Result<WriteToken, BackendError> {
            self.unsupported.write_plain_text(text)
        }

        fn source_name(&self) -> &'static str {
            self.unsupported.source_name()
        }
    }
}

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub use real::{WaylandBackend, WaylandProtocol, WaylandSupport};

#[cfg(any(not(target_os = "linux"), not(feature = "wayland")))]
pub use fallback::{WaylandBackend, WaylandProtocol, WaylandSupport};

#[cfg(all(test, target_os = "linux", feature = "wayland"))]
mod tests {
    use super::{WaylandBackend, WaylandProtocol, WaylandSupport};
    use crate::backend::BackendError;
    use crate::backend::ClipboardBackend;
    use std::collections::HashMap;
    use std::env;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_wayland_backend_reports_protocol_blocker_on_first_snapshot_read() {
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

    #[test]
    fn test_wayland_probe_requires_session_hint_for_automatic_mode() {
        let old_wayland = std::env::var_os("WAYLAND_DISPLAY");
        let old_session_type = std::env::var_os("XDG_SESSION_TYPE");
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::remove_var("XDG_SESSION_TYPE");

        assert_eq!(WaylandBackend::probe_support(), WaylandSupport::Unavailable);

        if let Some(value) = old_wayland {
            std::env::set_var("WAYLAND_DISPLAY", value);
        }
        if let Some(value) = old_session_type {
            std::env::set_var("XDG_SESSION_TYPE", value);
        }
    }

    #[test]
    fn test_wayland_protocols_are_marked_runtime_ready() {
        assert!(WaylandProtocol::ExtDataControl.runtime_implemented());
        assert!(WaylandProtocol::WlrDataControl.runtime_implemented());
    }

    #[test]
    fn test_wayland_prefers_ext_data_control_before_wlr() {
        let selected = WaylandBackend::select_protocol(true, true);
        assert_eq!(selected, Some(WaylandProtocol::ExtDataControl));
        assert_ne!(selected, Some(WaylandProtocol::WlrDataControl));
    }

    #[test]
    fn test_limited_wayland_backend_uses_cli_clipboard_tools() {
        let temp_dir = std::env::temp_dir().join(format!(
            "ccvv-linux-wayland-cli-tests-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).unwrap();
        let backup_path = env::var_os("PATH");
        let store = temp_dir.join("ccvv-wayland-fake-clipboard.txt");

        let wl_copy = temp_dir.join("wl-copy");
        let wl_paste = temp_dir.join("wl-paste");

        let copy_script = format!(
            "#!/bin/sh\nif [ \"${{1:-}}\" = \"--version\" ]; then exit 0; fi\ncat > {}\n",
            store.to_string_lossy()
        );
        let paste_script = format!(
            "#!/bin/sh\nif [ \"${{1:-}}\" = \"--version\" ]; then exit 0; fi\ncat {}\n",
            store.to_string_lossy()
        );

        fs::write(&wl_copy, copy_script.as_bytes()).unwrap();
        fs::write(&wl_paste, paste_script.as_bytes()).unwrap();

        let executable_mode = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&wl_copy, executable_mode.clone()).unwrap();
        fs::set_permissions(&wl_paste, executable_mode).unwrap();

        let mut test_path = temp_dir.to_string_lossy().to_string();
        if let Some(path) = backup_path.as_ref() {
            test_path.push(':');
            test_path.push_str(&path.to_string_lossy());
        }
        env::set_var("PATH", test_path);

        let mut backend = WaylandBackend::new_limited();
        backend.write_plain_text("clipboard payload").unwrap();
        let snapshot = backend.read_snapshot().unwrap();
        assert_eq!(snapshot.acquired_plain_text, "clipboard payload");

        match backup_path {
            Some(path) => env::set_var("PATH", path),
            None => env::remove_var("PATH"),
        }
        fs::remove_file(wl_copy).unwrap();
        fs::remove_file(wl_paste).unwrap();
        let _ = fs::remove_file(&store);
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_fallback_seat_id_is_stable_without_discovery() {
        let backend = WaylandBackend::new_limited();

        assert_eq!(
            backend.primary_seat_id(),
            super::real::FALLBACK_WAYLAND_SEAT
        );
    }

    #[test]
    fn test_select_text_mime_prefers_utf8_plain_text() {
        let mime = super::real::select_text_mime(&[
            "application/json".to_string(),
            "text/plain;charset=utf-8".to_string(),
            "text/plain".to_string(),
        ]);

        assert_eq!(mime.as_deref(), Some("text/plain;charset=utf-8"));
    }

    #[test]
    fn test_read_limited_bytes_rejects_oversize_payload() {
        let payload = vec![b'x'; 6];

        assert!(matches!(
            super::real::read_limited_bytes(payload.as_slice(), 5),
            Err(BackendError::Protocol(_))
        ));
    }

    #[test]
    fn test_take_self_write_flag_consumes_matching_text() {
        let by_seat = Arc::new(Mutex::new(HashMap::from([(
            "wayland-seat-1".to_string(),
            "payload".to_string(),
        )])));

        assert!(super::real::take_self_write_flag(
            &by_seat,
            "wayland-seat-1",
            "payload"
        ));
        assert!(!super::real::take_self_write_flag(
            &by_seat,
            "wayland-seat-1",
            "payload"
        ));
    }
}
