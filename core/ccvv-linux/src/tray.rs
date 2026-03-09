use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::mpsc;

use crate::control::socket::forward_command;
use crate::ui_protocol::{BackendMode, ControlCommand, StatusSnapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayIcon {
    Active,
    Paused,
    Limited,
    Error,
}

pub fn icon_for_status(status: &StatusSnapshot) -> TrayIcon {
    if status.paused {
        return TrayIcon::Paused;
    }
    if matches!(status.backend, BackendMode::Limited) {
        return TrayIcon::Limited;
    }
    if !status.last_clean_succeeded {
        return TrayIcon::Error;
    }

    TrayIcon::Active
}

pub fn freedesktop_icon_name(status: &StatusSnapshot) -> &'static str {
    match icon_for_status(status) {
        TrayIcon::Active => "edit-paste",
        TrayIcon::Paused => "media-playback-pause",
        TrayIcon::Limited => "dialog-information",
        TrayIcon::Error => "dialog-warning",
    }
}

pub fn title_for_status(status: &StatusSnapshot) -> String {
    let mode = match status.backend {
        BackendMode::X11 => "X11",
        BackendMode::Wayland => "Wayland",
        BackendMode::Limited => "Limited",
        BackendMode::None => "None",
    };

    if status.paused {
        return format!("ccvv (paused, {mode})");
    }
    if !status.last_clean_succeeded {
        return format!("ccvv (error, {mode})");
    }

    format!("ccvv ({mode})")
}

pub fn request_status(socket_path: &Path) -> Result<StatusSnapshot, TrayError> {
    let response = forward_command(socket_path, ControlCommand::GetStatus)?;
    StatusSnapshot::decode_line(response.trim()).ok_or(TrayError::InvalidStatus)
}

pub fn send_menu_action(socket_path: &Path, action: TrayAction) -> Result<(), TrayError> {
    let command = match action {
        TrayAction::Pause => ControlCommand::Pause,
        TrayAction::Resume => ControlCommand::Resume,
        TrayAction::CleanNow => ControlCommand::CleanNow,
        TrayAction::Quit => ControlCommand::Quit,
    };

    let response = forward_command(socket_path, command)?;
    if response.trim() == "ok" {
        return Ok(());
    }

    Err(TrayError::UnexpectedResponse(response))
}

pub fn watch_status_updates(
    socket_path: &Path,
    updates: mpsc::Sender<StatusSnapshot>,
) -> Result<(), TrayError> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket_path)?;
    stream.write_all(ControlCommand::SubscribeStatus.encode_line().as_bytes())?;
    stream.write_all(b"\n")?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }

        let snapshot = StatusSnapshot::decode_line(line.trim()).ok_or(TrayError::InvalidStatus)?;
        if updates.send(snapshot).is_err() {
            break;
        }
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayAction {
    Pause,
    Resume,
    CleanNow,
    Quit,
}

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error(transparent)]
    Runtime(#[from] crate::control::socket::RuntimeError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Service(String),
    #[error("invalid tray status response")]
    InvalidStatus,
    #[error("unexpected tray response: {0}")]
    UnexpectedResponse(String),
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::thread;

    use crate::control::socket::{bind_socket, prepare_runtime, RuntimePaths};
    use crate::tray::{
        freedesktop_icon_name, icon_for_status, request_status, send_menu_action, title_for_status,
        watch_status_updates, TrayAction, TrayIcon,
    };
    use crate::ui_protocol::{BackendCapability, BackendMode, StatusSnapshot};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "ccvv-linux-tray-{label}-{}-{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_status_mapping_prefers_paused_icon() {
        let status = StatusSnapshot {
            paused: true,
            backend: BackendMode::None,
            capability: BackendCapability::DiagnosticsOnly,
            last_clean_succeeded: true,
        };

        assert_eq!(icon_for_status(&status), TrayIcon::Paused);
    }

    #[test]
    fn test_title_and_icon_names_follow_status() {
        let status = StatusSnapshot {
            paused: false,
            backend: BackendMode::Limited,
            capability: BackendCapability::Limited,
            last_clean_succeeded: false,
        };

        assert_eq!(freedesktop_icon_name(&status), "dialog-information");
        assert_eq!(title_for_status(&status), "ccvv (error, Limited)");
    }

    #[test]
    fn test_request_status_parses_daemon_response() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);
        prepare_runtime(&paths).unwrap();
        let listener = bind_socket(&paths.socket_path).unwrap();
        let socket_path = paths.socket_path.clone();

        let handle = spawn_status_server(listener);
        let status = request_status(&socket_path).unwrap();

        assert_eq!(status.backend, BackendMode::None);
        assert!(status.last_clean_succeeded);
        handle.join().unwrap();
    }

    #[test]
    fn test_send_menu_action_requires_ok_response() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);
        prepare_runtime(&paths).unwrap();
        let listener = bind_socket(&paths.socket_path).unwrap();
        let socket_path = paths.socket_path.clone();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut payload = String::new();
            stream.read_to_string(&mut payload).unwrap();
            stream.write_all(b"ok\n").unwrap();
            payload
        });

        send_menu_action(&socket_path, TrayAction::CleanNow).unwrap();

        assert_eq!(handle.join().unwrap().trim(), "clean-now");
    }

    #[test]
    fn test_watch_status_updates_streams_subscription_snapshots() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);
        prepare_runtime(&paths).unwrap();
        let listener = bind_socket(&paths.socket_path).unwrap();
        let socket_path = paths.socket_path.clone();
        let (sender, receiver) = mpsc::channel();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut payload = String::new();
            stream.read_to_string(&mut payload).unwrap();
            assert_eq!(payload.trim(), "subscribe-status");

            for status in [
                StatusSnapshot {
                    paused: false,
                    backend: BackendMode::None,
                    capability: BackendCapability::DiagnosticsOnly,
                    last_clean_succeeded: true,
                },
                StatusSnapshot {
                    paused: true,
                    backend: BackendMode::Limited,
                    capability: BackendCapability::Limited,
                    last_clean_succeeded: true,
                },
            ] {
                stream.write_all(status.encode_line().as_bytes()).unwrap();
                stream.write_all(b"\n").unwrap();
            }
        });

        let watcher = thread::spawn(move || watch_status_updates(&socket_path, sender));

        assert!(!receiver.recv().unwrap().paused);
        assert!(receiver.recv().unwrap().paused);
        watcher.join().unwrap().unwrap();
        server.join().unwrap();
    }

    fn spawn_status_server(listener: UnixListener) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut payload = String::new();
            stream.read_to_string(&mut payload).unwrap();
            assert_eq!(payload.trim(), "get-status");

            let response = StatusSnapshot {
                paused: false,
                backend: BackendMode::None,
                capability: BackendCapability::DiagnosticsOnly,
                last_clean_succeeded: true,
            };

            stream.write_all(response.encode_line().as_bytes()).unwrap();
            stream.write_all(b"\n").unwrap();
        })
    }
}
