use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use thiserror::Error;

use ccvv_lib::config::{load_config, resolve_config, ResolvedConfig};
use ccvv_lib::history::HistoryDb;
use ccvv_lib::pipeline::Pipeline;
use ccvv_lib::CcvvError;

use crate::backend::none::NoneBackend;
use crate::backend::wayland::WaylandBackend;
use crate::backend::x11::X11Backend;
use crate::backend::{BackendError, ClipboardBackend, ClipboardSnapshot};
use crate::control::socket::{
    enforce_private_file_mode, prepare_runtime, serve, DaemonState, RuntimePaths,
};
use crate::detection::{DetectionOutcome, DetectorState};
use crate::single_instance::{acquire_single_instance, InstanceGuard};
use crate::ui_protocol::{BackendCapability, BackendMode, ControlCommand, StatusSnapshot};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendOverride {
    Auto,
    None,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeBackend {
    None,
    X11,
    Wayland,
    Limited,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppOptions {
    pub backend: BackendOverride,
    pub config_path: Option<PathBuf>,
    pub profile: Option<String>,
}

#[allow(dead_code)]
pub(crate) struct Bootstrap {
    pub paths: RuntimePaths,
    pub history: HistoryDb,
    pub warnings: Vec<String>,
    pub singleton: InstanceGuard,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] CcvvError),
    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error(transparent)]
    Runtime(#[from] crate::control::socket::RuntimeError),
    #[error(transparent)]
    SingleInstance(#[from] crate::single_instance::SingleInstanceError),
    #[error("internal thread panicked: {0}")]
    ThreadPanic(String),
}

pub fn run(options: AppOptions) -> Result<(), AppError> {
    let config = load_config(options.config_path.as_deref())?;
    let resolved = resolve_config(&config, options.profile.as_deref())?;
    let bootstrap = bootstrap(&options)?;

    let Bootstrap {
        warnings,
        history,
        singleton,
        ..
    } = bootstrap;

    let backend = select_backend(&options);
    let (listener, _socket_cleanup) = match singleton {
        InstanceGuard::Forwarded => {
            eprintln!("ccvv-linux: forwarded command to existing daemon");
            return Ok(());
        }
        InstanceGuard::Primary(listener, cleanup) => (listener, cleanup),
    };

    for warning in warnings {
        eprintln!("ccvv-linux: warning: {warning}");
    }

    let daemon_state = DaemonState::new(status_snapshot_for_backend(&backend));

    // Install signal handler for graceful shutdown
    let signal_quit = Arc::new(AtomicBool::new(false));
    let signal_quit_flag = signal_quit.clone();
    let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, signal_quit.clone());
    let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, signal_quit.clone());

    let (loop_errors_tx, loop_errors_rx) = mpsc::channel::<Result<(), AppError>>();
    let socket_state = daemon_state.clone();
    let loop_state = daemon_state.clone();
    let socket_join = thread::spawn(move || {
        let result = serve(listener, socket_state);
        let _ = loop_errors_tx.send(result.map_err(AppError::from));
    });

    let loop_result = run_clipboard_loop(
        backend,
        loop_state,
        &resolved,
        &history,
        &loop_errors_rx,
        &signal_quit_flag,
    );

    daemon_state.request_quit();

    if let Err(panic) = socket_join.join() {
        let msg = panic
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        return Err(AppError::ThreadPanic(format!(
            "control socket thread: {msg}"
        )));
    }

    if let Ok(error) = loop_errors_rx.try_recv() {
        return error;
    }

    loop_result
}

fn run_clipboard_loop(
    backend: RuntimeBackend,
    state: DaemonState,
    resolved: &ResolvedConfig,
    history: &HistoryDb,
    runtime_errors: &mpsc::Receiver<Result<(), AppError>>,
    signal_quit: &AtomicBool,
) -> Result<(), AppError> {
    let mut backend = build_backend(backend);
    let mut detector = DetectorState::new();
    let pipeline = Pipeline::from_resolved_config(resolved);
    let store_raw_history = resolved.settings.history_store_raw;
    let poll_interval = Duration::from_millis(250);
    let mut stream = backend.subscribe().ok();
    if stream.is_none() {
        eprintln!("ccvv-linux: backend subscription unavailable, using poll mode");
    }

    loop {
        if let Ok(error) = runtime_errors.try_recv() {
            return error;
        }

        if state.is_quitting() || signal_quit.load(Ordering::Relaxed) {
            state.request_quit();
            return Ok(());
        }

        if state.take_clean_request() {
            if let Ok(snapshot) = backend.read_snapshot() {
                match handle_snapshot(
                    &snapshot,
                    &pipeline,
                    history,
                    backend.as_mut(),
                    store_raw_history,
                    &state,
                ) {
                    Ok(_) => {}
                    Err(error) => eprintln!("ccvv-linux: clean-now failed: {error}"),
                }
            } else {
                state.set_last_clean_succeeded(false);
            }
        }

        if let Some(stream_rx) = stream.as_mut() {
            let mut should_close_stream = false;

            match stream_rx.recv_timeout(poll_interval) {
                Ok(result) => {
                    let snapshot = match result {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            if !matches!(error, BackendError::Unavailable) {
                                eprintln!("ccvv-linux: stream backend error: {error}");
                            }
                            continue;
                        }
                    };

                    if !state.is_paused()
                        && matches!(
                            detector.observe(&snapshot),
                            DetectionOutcome::TriggeredClean
                        )
                    {
                        match handle_snapshot(
                            &snapshot,
                            &pipeline,
                            history,
                            backend.as_mut(),
                            store_raw_history,
                            &state,
                        ) {
                            Ok(_) => {}
                            Err(error) => {
                                eprintln!("ccvv-linux: auto-clean failed: {error}");
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    should_close_stream = true;
                }
            };

            if should_close_stream {
                stream = None;
            }
        } else if !state.is_paused() {
            if let Ok(snapshot) = backend.read_snapshot() {
                if matches!(
                    detector.observe(&snapshot),
                    DetectionOutcome::TriggeredClean
                ) {
                    match handle_snapshot(
                        &snapshot,
                        &pipeline,
                        history,
                        backend.as_mut(),
                        store_raw_history,
                        &state,
                    ) {
                        Ok(_) => {}
                        Err(error) => {
                            eprintln!("ccvv-linux: auto-clean failed: {error}");
                        }
                    }
                }
            }

            thread::sleep(poll_interval);
        } else {
            thread::sleep(poll_interval);
        }
    }
}

fn handle_snapshot(
    snapshot: &ClipboardSnapshot,
    pipeline: &Pipeline,
    history: &HistoryDb,
    backend: &mut dyn ClipboardBackend,
    store_raw_history: bool,
    state: &DaemonState,
) -> Result<bool, AppError> {
    let (cleaned, context) = pipeline.run(&snapshot.acquired_plain_text);
    if cleaned == snapshot.acquired_plain_text {
        return Ok(false);
    }

    let entry_id = match history.prepare(
        &snapshot.acquired_plain_text,
        &cleaned,
        context.content_type,
        store_raw_history,
    ) {
        Ok(id) => id,
        Err(error) => {
            state.set_last_clean_succeeded(false);
            eprintln!("ccvv-linux: failed to prepare history entry: {error}");
            return Err(AppError::from(error));
        }
    };

    if let Err(error) = backend.write_plain_text(&cleaned) {
        state.set_last_clean_succeeded(false);
        if let Err(rollback_error) = history.rollback_entry(entry_id) {
            eprintln!("ccvv-linux: failed to rollback history entry: {rollback_error}");
        }
        eprintln!("ccvv-linux: clipboard write failed: {error}");
        return Ok(false);
    }

    if let Err(error) = history.commit_entry(entry_id) {
        state.set_last_clean_succeeded(false);
        return Err(AppError::from(error));
    }

    state.set_last_clean_succeeded(true);
    Ok(true)
}

fn build_backend(backend: RuntimeBackend) -> Box<dyn ClipboardBackend> {
    match backend {
        RuntimeBackend::None => Box::new(NoneBackend::new()),
        RuntimeBackend::X11 => Box::new(X11Backend::new()),
        RuntimeBackend::Wayland => Box::new(WaylandBackend::new()),
        RuntimeBackend::Limited => Box::new(WaylandBackend::new_limited()),
    }
}

fn status_snapshot_for_backend(backend: &RuntimeBackend) -> StatusSnapshot {
    let (backend, capability) = match backend {
        RuntimeBackend::None => (BackendMode::None, BackendCapability::DiagnosticsOnly),
        RuntimeBackend::X11 => (BackendMode::X11, BackendCapability::Automatic),
        RuntimeBackend::Wayland => (BackendMode::Wayland, BackendCapability::Automatic),
        RuntimeBackend::Limited => (BackendMode::Limited, BackendCapability::Limited),
    };

    StatusSnapshot {
        paused: false,
        backend,
        capability,
        last_clean_succeeded: true,
    }
}

fn bootstrap(options: &AppOptions) -> Result<Bootstrap, AppError> {
    let paths = RuntimePaths::from_env(options.config_path.clone())?;
    let mut warnings = prepare_runtime(&paths)?;
    let history = HistoryDb::open(&paths.history_path)?;
    enforce_private_file_mode(&paths.history_path, &mut warnings)?;
    let singleton = acquire_single_instance(&paths.socket_path, ControlCommand::GetStatus)?;

    Ok(Bootstrap {
        paths,
        history,
        warnings,
        singleton,
    })
}

fn select_backend(options: &AppOptions) -> RuntimeBackend {
    match options.backend {
        BackendOverride::None => RuntimeBackend::None,
        BackendOverride::Auto => {
            if is_gnome_wayland_session() {
                return RuntimeBackend::Limited;
            }

            if is_wayland_session() {
                return RuntimeBackend::Wayland;
            }

            if is_x11_session() {
                return RuntimeBackend::X11;
            }

            RuntimeBackend::None
        }
    }
}

fn is_wayland_session() -> bool {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return true;
    }

    matches!(
        std::env::var("XDG_SESSION_TYPE")
            .unwrap_or_else(|_| String::new())
            .to_lowercase()
            .as_str(),
        "wayland" | "wayland-only"
    )
}

fn is_x11_session() -> bool {
    std::env::var_os("DISPLAY").is_some()
}

fn is_gnome_wayland_session() -> bool {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_else(|_| String::new())
        .to_lowercase();
    let session = std::env::var("XDG_SESSION_DESKTOP")
        .unwrap_or_else(|_| String::new())
        .to_lowercase();

    is_wayland_session()
        && (desktop.contains("gnome")
            || session.contains("gnome")
            || std::env::var("GDMSESSION").is_ok_and(|value| value.eq_ignore_ascii_case("gnome")))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{
        bootstrap, handle_snapshot, is_gnome_wayland_session, is_wayland_session, is_x11_session,
        run, select_backend, status_snapshot_for_backend, AppOptions, BackendOverride,
        RuntimeBackend,
    };
    use crate::backend::{ClipboardBackend, ClipboardSnapshot, SelectionKind, WriteToken};
    use crate::control::socket::forward_command;
    use crate::control::socket::DaemonState;
    use crate::ui_protocol::{BackendCapability, BackendMode, ControlCommand, StatusSnapshot};
    use ccvv_lib::history::HistoryDb;
    use ccvv_lib::pipeline::Pipeline;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "ccvv-linux-app-{label}-{}-{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_bootstrap_sets_history_db_mode_to_0600() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let old_home = std::env::var_os("HOME");
        let old_runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
        std::env::set_var("HOME", &home_dir);
        std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);

        let bootstrap = bootstrap(&AppOptions {
            backend: BackendOverride::None,
            config_path: None,
            profile: None,
        })
        .unwrap();

        let mode = fs::metadata(&bootstrap.paths.history_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(mode, 0o600);
        restore_env_var("HOME", old_home);
        restore_env_var("XDG_RUNTIME_DIR", old_runtime_dir);
    }

    #[test]
    fn test_bootstrap_accepts_custom_config_parent_creation() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let config_path = home_dir.join("nested").join("config").join("config.toml");
        let old_home = std::env::var_os("HOME");
        let old_runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
        std::env::set_var("HOME", &home_dir);
        std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);

        let bootstrap = bootstrap(&AppOptions {
            backend: BackendOverride::None,
            config_path: Some(config_path.clone()),
            profile: None,
        })
        .unwrap();

        assert!(config_path.exists());
        assert_eq!(bootstrap.paths.config_path, config_path);
        restore_env_var("HOME", old_home);
        restore_env_var("XDG_RUNTIME_DIR", old_runtime_dir);
    }

    #[test]
    fn test_select_backend_prefers_wayland_over_x11_when_wayland_is_detected() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_wayland = std::env::var_os("WAYLAND_DISPLAY");
        let old_display = std::env::var_os("DISPLAY");
        let old_current_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
        let old_session_desktop = std::env::var_os("XDG_SESSION_DESKTOP");
        let old_gdmsession = std::env::var_os("GDMSESSION");
        let old_session_type = std::env::var_os("XDG_SESSION_TYPE");

        std::env::set_var("WAYLAND_DISPLAY", ":0");
        std::env::set_var("DISPLAY", ":1");
        std::env::set_var("XDG_CURRENT_DESKTOP", "KDE");
        std::env::set_var("XDG_SESSION_DESKTOP", "KDE");
        std::env::remove_var("GDMSESSION");
        std::env::remove_var("XDG_SESSION_TYPE");

        assert_eq!(
            select_backend(&AppOptions {
                backend: super::BackendOverride::Auto,
                config_path: None,
                profile: None,
            }),
            RuntimeBackend::Wayland
        );

        restore_env_var("WAYLAND_DISPLAY", old_wayland);
        restore_env_var("DISPLAY", old_display);
        restore_env_var("XDG_CURRENT_DESKTOP", old_current_desktop);
        restore_env_var("XDG_SESSION_DESKTOP", old_session_desktop);
        restore_env_var("GDMSESSION", old_gdmsession);
        restore_env_var("XDG_SESSION_TYPE", old_session_type);
    }

    #[test]
    fn test_select_backend_uses_limited_mode_for_gnome_wayland() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_wayland = std::env::var_os("WAYLAND_DISPLAY");
        let old_display = std::env::var_os("DISPLAY");
        let old_current_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
        let old_session_desktop = std::env::var_os("XDG_SESSION_DESKTOP");
        let old_gdmsession = std::env::var_os("GDMSESSION");
        let old_session_type = std::env::var_os("XDG_SESSION_TYPE");

        std::env::set_var("WAYLAND_DISPLAY", ":0");
        std::env::set_var("XDG_CURRENT_DESKTOP", "GNOME");

        assert_eq!(
            select_backend(&AppOptions {
                backend: super::BackendOverride::Auto,
                config_path: None,
                profile: None,
            }),
            RuntimeBackend::Limited
        );

        restore_env_var("WAYLAND_DISPLAY", old_wayland);
        restore_env_var("DISPLAY", old_display);
        restore_env_var("XDG_CURRENT_DESKTOP", old_current_desktop);
        restore_env_var("XDG_SESSION_DESKTOP", old_session_desktop);
        restore_env_var("GDMSESSION", old_gdmsession);
        restore_env_var("XDG_SESSION_TYPE", old_session_type);
    }

    #[test]
    fn test_select_backend_falls_back_to_x11_when_display_is_set_without_wayland() {
        let _guard = ENV_LOCK.lock().unwrap();
        let old_wayland = std::env::var_os("WAYLAND_DISPLAY");
        let old_display = std::env::var_os("DISPLAY");
        let old_current_desktop = std::env::var_os("XDG_CURRENT_DESKTOP");
        let old_session_desktop = std::env::var_os("XDG_SESSION_DESKTOP");
        let old_gdmsession = std::env::var_os("GDMSESSION");
        let old_session_type = std::env::var_os("XDG_SESSION_TYPE");

        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::set_var("DISPLAY", ":1");

        assert_eq!(
            select_backend(&AppOptions {
                backend: super::BackendOverride::Auto,
                config_path: None,
                profile: None,
            }),
            RuntimeBackend::X11
        );

        restore_env_var("WAYLAND_DISPLAY", old_wayland);
        restore_env_var("DISPLAY", old_display);
        restore_env_var("XDG_CURRENT_DESKTOP", old_current_desktop);
        restore_env_var("XDG_SESSION_DESKTOP", old_session_desktop);
        restore_env_var("GDMSESSION", old_gdmsession);
        restore_env_var("XDG_SESSION_TYPE", old_session_type);
    }

    #[test]
    fn test_status_snapshot_reflects_backend_selection() {
        assert_eq!(
            status_snapshot_for_backend(&RuntimeBackend::X11),
            StatusSnapshot {
                paused: false,
                backend: BackendMode::X11,
                capability: BackendCapability::Automatic,
                last_clean_succeeded: true,
            }
        );
        assert_eq!(
            status_snapshot_for_backend(&RuntimeBackend::Wayland),
            StatusSnapshot {
                paused: false,
                backend: BackendMode::Wayland,
                capability: BackendCapability::Automatic,
                last_clean_succeeded: true,
            }
        );
        assert_eq!(
            status_snapshot_for_backend(&RuntimeBackend::Limited),
            StatusSnapshot {
                paused: false,
                backend: BackendMode::Limited,
                capability: BackendCapability::Limited,
                last_clean_succeeded: true,
            }
        );
        assert_eq!(
            status_snapshot_for_backend(&RuntimeBackend::None),
            StatusSnapshot {
                paused: false,
                backend: BackendMode::None,
                capability: BackendCapability::DiagnosticsOnly,
                last_clean_succeeded: true,
            }
        );
    }

    #[test]
    fn test_detects_wayland_and_gnome_session_via_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        restore_env_var("WAYLAND_DISPLAY", None);
        restore_env_var("DISPLAY", None);
        restore_env_var("XDG_SESSION_TYPE", None);
        restore_env_var("XDG_CURRENT_DESKTOP", None);
        restore_env_var("XDG_SESSION_DESKTOP", None);
        restore_env_var("GDMSESSION", None);

        assert!(!is_wayland_session());
        assert!(!is_x11_session());
        assert!(!is_gnome_wayland_session());

        std::env::set_var("WAYLAND_DISPLAY", "wayland-0");
        std::env::set_var("XDG_SESSION_DESKTOP", "gnome");
        assert!(is_wayland_session());
        assert!(is_gnome_wayland_session());
        assert!(!is_x11_session());

        restore_env_var("WAYLAND_DISPLAY", None);
        restore_env_var("XDG_SESSION_DESKTOP", None);
        restore_env_var("XDG_CURRENT_DESKTOP", Some("GNOME".into()));
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
        std::env::set_var("WAYLAND_DISPLAY", "wayland-1");
        assert!(is_wayland_session());
        assert!(is_gnome_wayland_session());

        restore_env_var("WAYLAND_DISPLAY", None);
        restore_env_var("XDG_SESSION_TYPE", None);
        restore_env_var("XDG_CURRENT_DESKTOP", None);
        restore_env_var("XDG_SESSION_DESKTOP", None);
        restore_env_var("GDMSESSION", None);
    }

    #[test]
    fn test_run_keeps_socket_available_for_status_requests_until_quit() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let old_home = std::env::var_os("HOME");
        let old_runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
        std::env::set_var("HOME", &home_dir);
        std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);

        let handle = spawn_daemon_thread();
        let socket_path = runtime_dir.join("ccvv.sock");
        wait_for_socket(&socket_path);

        let status = wait_for_status(&socket_path);
        assert_eq!(
            status,
            StatusSnapshot {
                paused: false,
                backend: BackendMode::None,
                capability: BackendCapability::DiagnosticsOnly,
                last_clean_succeeded: true,
            }
        );

        forward_command(&socket_path, ControlCommand::Quit).unwrap();
        handle.join().unwrap().unwrap();
        restore_env_var("HOME", old_home);
        restore_env_var("XDG_RUNTIME_DIR", old_runtime_dir);
    }

    #[test]
    fn test_subscribe_status_receives_state_changes() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let old_home = std::env::var_os("HOME");
        let old_runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
        std::env::set_var("HOME", &home_dir);
        std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);

        let handle = spawn_daemon_thread();
        let socket_path = runtime_dir.join("ccvv.sock");
        wait_for_socket(&socket_path);

        let mut stream = UnixStream::connect(&socket_path).unwrap();
        stream.write_all(b"subscribe-status\n").unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut reader = BufReader::new(stream);

        let initial = read_status_line(&mut reader);
        assert!(!initial.paused);

        forward_command(&socket_path, ControlCommand::Pause).unwrap();
        let updated = read_status_line(&mut reader);
        assert!(updated.paused);

        forward_command(&socket_path, ControlCommand::Quit).unwrap();
        handle.join().unwrap().unwrap();
        restore_env_var("HOME", old_home);
        restore_env_var("XDG_RUNTIME_DIR", old_runtime_dir);
    }

    #[test]
    fn test_clean_now_marks_last_clean_as_failed_when_snapshot_is_unavailable() {
        let _guard = ENV_LOCK.lock().unwrap();
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let old_home = std::env::var_os("HOME");
        let old_runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
        std::env::set_var("HOME", &home_dir);
        std::env::set_var("XDG_RUNTIME_DIR", &runtime_dir);

        let handle = spawn_daemon_thread();
        let socket_path = runtime_dir.join("ccvv.sock");
        wait_for_socket(&socket_path);

        forward_command(&socket_path, ControlCommand::CleanNow).unwrap();
        let observed =
            wait_for_status_with_predicate(&socket_path, |snapshot| !snapshot.last_clean_succeeded);
        assert!(!observed.last_clean_succeeded);

        forward_command(&socket_path, ControlCommand::Quit).unwrap();
        handle.join().unwrap().unwrap();
        restore_env_var("HOME", old_home);
        restore_env_var("XDG_RUNTIME_DIR", old_runtime_dir);
    }

    #[test]
    fn test_handle_snapshot_rolls_back_history_when_write_fails() {
        let history_path = temp_dir("history").join("history.db");
        let history = HistoryDb::open(&history_path).unwrap();
        let state = DaemonState::new(StatusSnapshot {
            paused: false,
            backend: BackendMode::None,
            capability: BackendCapability::DiagnosticsOnly,
            last_clean_succeeded: true,
        });

        let resolved = ccvv_lib::config::ResolvedConfig::default();
        let snapshot = ClipboardSnapshot {
            seat_id: "seat0".to_string(),
            selection_kind: SelectionKind::Clipboard,
            acquired_plain_text: "hello    world".to_string(),
            acquired_html: Some("hello".to_string()),
            timestamp: 1,
            backend_serial: Some(1),
            is_self_write: false,
        };
        let pipeline = Pipeline::from_resolved_config(&resolved);
        let mut backend = FailingBackend;
        let initial = history.recent(10).unwrap().len();

        let result =
            handle_snapshot(&snapshot, &pipeline, &history, &mut backend, false, &state).unwrap();

        assert!(!result);
        assert!(!state.snapshot().last_clean_succeeded);
        assert_eq!(history.recent(10).unwrap().len(), initial);
    }

    struct FailingBackend;

    impl ClipboardBackend for FailingBackend {
        fn capability(&self) -> crate::ui_protocol::BackendCapability {
            BackendCapability::DiagnosticsOnly
        }

        fn read_snapshot(&mut self) -> Result<ClipboardSnapshot, crate::backend::BackendError> {
            Err(crate::backend::BackendError::Unavailable)
        }

        fn write_plain_text(
            &mut self,
            _text: &str,
        ) -> Result<WriteToken, crate::backend::BackendError> {
            Err(crate::backend::BackendError::Unavailable)
        }

        fn source_name(&self) -> &'static str {
            "test-failing-backend"
        }
    }

    fn spawn_daemon_thread() -> thread::JoinHandle<Result<(), super::AppError>> {
        thread::spawn(|| {
            run(AppOptions {
                backend: BackendOverride::None,
                config_path: None,
                profile: None,
            })
        })
    }

    fn wait_for_socket(socket_path: &std::path::Path) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if socket_path.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }

        panic!("socket did not appear: {}", socket_path.display());
    }

    fn wait_for_status(socket_path: &std::path::Path) -> StatusSnapshot {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut last_error = None;

        while Instant::now() < deadline {
            match forward_command(socket_path, ControlCommand::GetStatus) {
                Ok(response) => {
                    return StatusSnapshot::decode_line(response.trim())
                        .expect("status response should decode");
                }
                Err(error) => {
                    last_error = Some(error);
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }

        panic!("status request never succeeded: {last_error:?}");
    }

    fn wait_for_status_with_predicate(
        socket_path: &std::path::Path,
        predicate: impl Fn(StatusSnapshot) -> bool,
    ) -> StatusSnapshot {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut last_error = None;

        while Instant::now() < deadline {
            match forward_command(socket_path, ControlCommand::GetStatus) {
                Ok(response) => {
                    let snapshot = StatusSnapshot::decode_line(response.trim())
                        .expect("status response should decode");
                    if predicate(snapshot.clone()) {
                        return snapshot;
                    }
                }
                Err(error) => last_error = Some(error),
            }

            thread::sleep(Duration::from_millis(10));
        }

        panic!("status predicate never satisfied: {last_error:?}");
    }

    fn read_status_line(reader: &mut BufReader<UnixStream>) -> StatusSnapshot {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        StatusSnapshot::decode_line(line.trim()).expect("status response should decode")
    }

    fn restore_env_var(key: &str, value: Option<std::ffi::OsString>) {
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
}
