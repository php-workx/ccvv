use std::path::Path;

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
    use std::thread;

    use crate::control::socket::{bind_socket, prepare_runtime, RuntimePaths};
    use crate::tray::{icon_for_status, request_status, send_menu_action, TrayAction, TrayIcon};
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
