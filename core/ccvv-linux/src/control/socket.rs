#![allow(dead_code)]

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

use crate::ui_protocol::{ControlCommand, StatusSnapshot};

const DIRECTORY_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimePaths {
    pub state_dir: PathBuf,
    pub config_path: PathBuf,
    pub history_path: PathBuf,
    pub socket_path: PathBuf,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("HOME environment variable not set")]
    MissingHome,
    #[error("XDG_RUNTIME_DIR environment variable not set")]
    MissingRuntimeDir,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("invalid control command on socket")]
    InvalidCommand,
    #[error("control socket did not acknowledge the command")]
    MissingAcknowledgement,
}

impl RuntimePaths {
    pub fn from_env(config_path: Option<PathBuf>) -> Result<Self, RuntimeError> {
        let home = std::env::var_os("HOME").ok_or(RuntimeError::MissingHome)?;
        let runtime_dir =
            std::env::var_os("XDG_RUNTIME_DIR").ok_or(RuntimeError::MissingRuntimeDir)?;

        Ok(Self::from_roots(
            PathBuf::from(home),
            PathBuf::from(runtime_dir),
            config_path,
        ))
    }

    pub fn from_roots(
        home_dir: PathBuf,
        runtime_dir: PathBuf,
        config_path: Option<PathBuf>,
    ) -> Self {
        let state_dir = home_dir.join(".ccvv");
        let config_path = config_path.unwrap_or_else(|| state_dir.join("config.toml"));
        let history_path = state_dir.join("history.db");
        let socket_path = runtime_dir.join("ccvv.sock");

        Self {
            state_dir,
            config_path,
            history_path,
            socket_path,
        }
    }
}

pub fn prepare_runtime(paths: &RuntimePaths) -> Result<Vec<String>, RuntimeError> {
    let mut warnings = Vec::new();

    ensure_directory_mode(&paths.state_dir, DIRECTORY_MODE)?;
    ensure_file_parent(&paths.config_path)?;
    ensure_file_mode(&paths.config_path, FILE_MODE, true, &mut warnings)?;
    ensure_socket_parent(&paths.socket_path)?;

    Ok(warnings)
}

pub fn bind_socket(path: &Path) -> Result<UnixListener, RuntimeError> {
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(FILE_MODE))?;
    Ok(listener)
}

pub fn read_command(stream: &mut UnixStream) -> Result<ControlCommand, RuntimeError> {
    let mut buffer = String::new();
    stream.read_to_string(&mut buffer)?;
    ControlCommand::decode_line(buffer.trim()).ok_or(RuntimeError::InvalidCommand)
}

pub fn send_command(path: &Path, command: ControlCommand) -> Result<(), RuntimeError> {
    let mut stream = UnixStream::connect(path)?;
    stream.write_all(command.encode_line().as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}

pub fn forward_command(path: &Path, command: ControlCommand) -> Result<String, RuntimeError> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_millis(250)))?;
    stream.write_all(command.encode_line().as_bytes())?;
    stream.write_all(b"\n")?;
    stream.shutdown(Shutdown::Write)?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if response.trim().is_empty() {
        return Err(RuntimeError::MissingAcknowledgement);
    }

    Ok(response)
}

pub fn status_line(snapshot: &StatusSnapshot) -> String {
    snapshot.encode_line()
}

fn ensure_socket_parent(socket_path: &Path) -> Result<(), RuntimeError> {
    let parent = socket_path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "socket path has no parent",
        )
    })?;

    fs::create_dir_all(parent)?;
    Ok(())
}

fn ensure_file_parent(path: &Path) -> Result<(), RuntimeError> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "file path has no parent")
    })?;
    fs::create_dir_all(parent)?;
    Ok(())
}

fn ensure_directory_mode(path: &Path, mode: u32) -> Result<(), RuntimeError> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn ensure_file_mode(
    path: &Path,
    mode: u32,
    create_if_missing: bool,
    warnings: &mut Vec<String>,
) -> Result<(), RuntimeError> {
    if !path.exists() {
        if create_if_missing {
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(false)
                .open(path)?;
            file.set_permissions(fs::Permissions::from_mode(mode))?;
        } else {
            return Ok(());
        }
    }

    let current_mode = fs::metadata(path)?.permissions().mode() & 0o777;
    if current_mode != mode {
        match fs::set_permissions(path, fs::Permissions::from_mode(mode)) {
            Ok(()) => warnings.push(format!(
                "repaired permissions for {} from {:o} to {:o}",
                path.display(),
                current_mode,
                mode
            )),
            Err(error) => warnings.push(format!(
                "failed to repair permissions for {}: {}",
                path.display(),
                error
            )),
        }
    }

    Ok(())
}

pub fn enforce_private_file_mode(
    path: &Path,
    warnings: &mut Vec<String>,
) -> Result<(), RuntimeError> {
    ensure_file_mode(path, FILE_MODE, false, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;

    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "ccvv-linux-{label}-{}-{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_ccvv_dir_created_with_0700() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);

        let warnings = prepare_runtime(&paths).unwrap();

        assert!(warnings.is_empty());
        let mode = fs::metadata(&paths.state_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, DIRECTORY_MODE);
    }

    #[test]
    fn test_runtime_socket_mode_is_0600() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);

        prepare_runtime(&paths).unwrap();
        let listener = bind_socket(&paths.socket_path).unwrap();
        let mode = fs::metadata(&paths.socket_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;

        drop(listener);
        assert_eq!(mode, FILE_MODE);
    }

    #[test]
    fn test_permission_repair_or_warn_on_broad_config_mode() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);

        fs::create_dir_all(&paths.state_dir).unwrap();
        fs::write(&paths.config_path, "").unwrap();
        fs::set_permissions(&paths.config_path, fs::Permissions::from_mode(0o644)).unwrap();

        let warnings = prepare_runtime(&paths).unwrap();
        let mode = fs::metadata(&paths.config_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(mode, FILE_MODE);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_custom_config_parent_dirs_are_created() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let custom_config = home_dir.join("nested").join("more").join("config.toml");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, Some(custom_config.clone()));

        prepare_runtime(&paths).unwrap();

        assert!(custom_config.exists());
        assert!(custom_config.parent().unwrap().exists());
    }

    #[test]
    fn test_read_command_round_trip() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);

        prepare_runtime(&paths).unwrap();
        let listener = bind_socket(&paths.socket_path).unwrap();
        let socket_path = paths.socket_path.clone();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_command(&mut stream).unwrap()
        });

        send_command(&socket_path, ControlCommand::CleanNow).unwrap();

        assert_eq!(handle.join().unwrap(), ControlCommand::CleanNow);
    }

    #[test]
    fn test_forward_command_requires_response() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);

        prepare_runtime(&paths).unwrap();
        let listener = bind_socket(&paths.socket_path).unwrap();
        let socket_path = paths.socket_path.clone();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_command(&mut stream).unwrap();
        });

        let error = forward_command(&socket_path, ControlCommand::CleanNow).unwrap_err();

        assert!(matches!(error, RuntimeError::MissingAcknowledgement));
        handle.join().unwrap();
    }
}
