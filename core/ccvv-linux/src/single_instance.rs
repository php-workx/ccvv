#![allow(dead_code)]

use std::fs;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::control::socket::{bind_socket, forward_command, RuntimeError};
use crate::ui_protocol::ControlCommand;

/// Removes the socket file on drop for clean shutdown.
#[derive(Debug)]
pub struct SocketCleanup {
    path: PathBuf,
}

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
pub enum InstanceGuard {
    Primary(UnixListener, SocketCleanup),
    Forwarded,
}

#[derive(Debug, Error)]
pub enum SingleInstanceError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn acquire_single_instance(
    socket_path: &Path,
    command: ControlCommand,
) -> Result<InstanceGuard, SingleInstanceError> {
    // Try bind first (eliminates TOCTOU race)
    match bind_socket(socket_path) {
        Ok(listener) => {
            let cleanup = SocketCleanup {
                path: socket_path.to_path_buf(),
            };
            return Ok(InstanceGuard::Primary(listener, cleanup));
        }
        Err(RuntimeError::Io(ref error)) if error.kind() == std::io::ErrorKind::AddrInUse => {
            // Socket exists — try to forward
        }
        Err(error) => return Err(error.into()),
    }

    // Socket exists — try to talk to the existing daemon
    match forward_command(socket_path, command) {
        Ok(_) => Ok(InstanceGuard::Forwarded),
        Err(RuntimeError::Io(_)) | Err(RuntimeError::MissingAcknowledgement) => {
            // Stale socket — remove and rebind
            fs::remove_file(socket_path)?;
            let listener = bind_socket(socket_path)?;
            let cleanup = SocketCleanup {
                path: socket_path.to_path_buf(),
            };
            Ok(InstanceGuard::Primary(listener, cleanup))
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    use crate::control::socket::RuntimePaths;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "ccvv-linux-single-instance-{label}-{}-{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_stale_socket_is_rebound() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);

        fs::create_dir_all(paths.socket_path.parent().unwrap()).unwrap();
        fs::File::create(&paths.socket_path).unwrap();

        let guard = acquire_single_instance(&paths.socket_path, ControlCommand::GetStatus).unwrap();

        assert!(matches!(guard, InstanceGuard::Primary(_, _)));
    }

    #[test]
    fn test_second_instance_forwards_command() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir.clone(), None);
        fs::create_dir_all(&runtime_dir).unwrap();

        let guard = acquire_single_instance(&paths.socket_path, ControlCommand::GetStatus).unwrap();
        let (listener, _cleanup) = match guard {
            InstanceGuard::Primary(listener, cleanup) => (listener, cleanup),
            InstanceGuard::Forwarded => panic!("first instance should bind"),
        };

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut payload = String::new();
            stream.read_to_string(&mut payload).unwrap();
            stream.write_all(b"ok\n").unwrap();
            payload
        });

        let second = acquire_single_instance(&paths.socket_path, ControlCommand::Pause).unwrap();

        assert!(matches!(second, InstanceGuard::Forwarded));
        assert_eq!(handle.join().unwrap().trim(), "pause");
    }

    #[test]
    fn test_foreign_listener_without_ack_is_not_treated_as_healthy_daemon() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);
        fs::create_dir_all(paths.socket_path.parent().unwrap()).unwrap();

        let foreign = UnixListener::bind(&paths.socket_path).unwrap();
        let handle = thread::spawn(move || {
            let (_stream, _) = foreign.accept().unwrap();
        });

        let guard = acquire_single_instance(&paths.socket_path, ControlCommand::GetStatus).unwrap();

        assert!(matches!(guard, InstanceGuard::Primary(_, _)));
        handle.join().unwrap();
    }

    #[test]
    fn test_drop_removes_socket_file() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);
        fs::create_dir_all(paths.socket_path.parent().unwrap()).unwrap();

        let guard = acquire_single_instance(&paths.socket_path, ControlCommand::GetStatus).unwrap();
        assert!(paths.socket_path.exists());
        drop(guard);
        assert!(!paths.socket_path.exists());
    }
}
