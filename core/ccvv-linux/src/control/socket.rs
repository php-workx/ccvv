#![allow(dead_code)]

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

const MAX_COMMAND_BYTES: usize = 4096;
const MAX_CONCURRENT_CLIENTS: usize = 16;

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
    stream
        .take(MAX_COMMAND_BYTES as u64)
        .read_to_string(&mut buffer)?;
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
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(command.encode_line().as_bytes())?;
    stream.write_all(b"\n")?;
    stream.shutdown(Shutdown::Write)?;

    let mut response = String::new();
    stream
        .take(MAX_COMMAND_BYTES as u64)
        .read_to_string(&mut response)?;
    if response.trim().is_empty() {
        return Err(RuntimeError::MissingAcknowledgement);
    }

    Ok(response)
}

pub fn status_line(snapshot: &StatusSnapshot) -> String {
    snapshot.encode_line()
}

#[derive(Clone, Debug)]
pub struct DaemonState {
    inner: Arc<DaemonStateInner>,
}

#[derive(Debug)]
struct DaemonStateInner {
    status: Mutex<StatusSnapshot>,
    subscribers: Mutex<Vec<mpsc::Sender<StatusSnapshot>>>,
    quitting: AtomicBool,
    clean_requested: AtomicBool,
}

impl DaemonState {
    pub fn new(status: StatusSnapshot) -> Self {
        Self {
            inner: Arc::new(DaemonStateInner {
                status: Mutex::new(status),
                subscribers: Mutex::new(Vec::new()),
                quitting: AtomicBool::new(false),
                clean_requested: AtomicBool::new(false),
            }),
        }
    }

    pub fn snapshot(&self) -> StatusSnapshot {
        self.inner
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn is_quitting(&self) -> bool {
        self.inner.quitting.load(Ordering::SeqCst)
    }

    pub fn is_paused(&self) -> bool {
        self.inner
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .paused
    }

    pub fn set_last_clean_succeeded(&self, succeeded: bool) {
        self.update_status(|status| status.last_clean_succeeded = succeeded);
    }

    fn subscribe(&self) -> mpsc::Receiver<StatusSnapshot> {
        let (sender, receiver) = mpsc::channel();
        sender.send(self.snapshot()).ok();
        self.inner
            .subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(sender);
        receiver
    }

    fn update_status<F>(&self, update: F) -> StatusSnapshot
    where
        F: FnOnce(&mut StatusSnapshot),
    {
        let snapshot = {
            let mut status = self
                .inner
                .status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            update(&mut status);
            status.clone()
        };
        self.broadcast(snapshot.clone());
        snapshot
    }

    pub fn request_quit(&self) {
        self.inner.quitting.store(true, Ordering::SeqCst);
    }

    pub fn request_clean_now(&self) {
        self.inner.clean_requested.store(true, Ordering::SeqCst);
    }

    pub fn take_clean_request(&self) -> bool {
        self.inner.clean_requested.swap(false, Ordering::SeqCst)
    }

    fn broadcast(&self, snapshot: StatusSnapshot) {
        let mut subscribers = self
            .inner
            .subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        subscribers.retain(|sender| sender.send(snapshot.clone()).is_ok());
    }
}

struct ConnectionGuard {
    counter: Arc<AtomicUsize>,
}

impl ConnectionGuard {
    fn acquire(counter: &Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self {
            counter: counter.clone(),
        }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn serve(listener: UnixListener, state: DaemonState) -> Result<(), RuntimeError> {
    listener.set_nonblocking(true)?;
    let active_clients = Arc::new(AtomicUsize::new(0));

    while !state.is_quitting() {
        match listener.accept() {
            Ok((stream, _)) => {
                if active_clients.load(Ordering::SeqCst) >= MAX_CONCURRENT_CLIENTS {
                    drop(stream);
                    continue;
                }
                let state = state.clone();
                let guard = ConnectionGuard::acquire(&active_clients);
                thread::spawn(move || {
                    let _guard = guard;
                    if let Err(error) = handle_client(stream, state) {
                        eprintln!("ccvv-linux: client handler error: {error}");
                    }
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(RuntimeError::Io(error)),
        }
    }

    Ok(())
}

fn handle_client(mut stream: UnixStream, state: DaemonState) -> Result<(), RuntimeError> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    match read_command(&mut stream)? {
        ControlCommand::GetStatus => write_status(&mut stream, &state.snapshot())?,
        ControlCommand::Pause => {
            state.update_status(|status| status.paused = true);
            write_ok(&mut stream)?;
        }
        ControlCommand::Resume => {
            state.update_status(|status| status.paused = false);
            write_ok(&mut stream)?;
        }
        ControlCommand::CleanNow => {
            state.request_clean_now();
            write_ok(&mut stream)?;
        }
        ControlCommand::Quit => {
            state.request_quit();
            write_ok(&mut stream)?;
        }
        ControlCommand::SubscribeStatus => {
            stream.set_write_timeout(Some(Duration::from_secs(5)))?;
            let receiver = state.subscribe();
            for snapshot in receiver {
                write_status(&mut stream, &snapshot)?;
            }
        }
    }

    Ok(())
}

fn write_ok(stream: &mut UnixStream) -> Result<(), RuntimeError> {
    stream.write_all(b"ok\n")?;
    Ok(())
}

fn write_status(stream: &mut UnixStream, snapshot: &StatusSnapshot) -> Result<(), RuntimeError> {
    stream.write_all(status_line(snapshot).as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
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
    if create_if_missing {
        match OpenOptions::new().create_new(true).write(true).open(path) {
            Ok(file) => {
                file.set_permissions(fs::Permissions::from_mode(mode))?;
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // fall through to permission check
            }
            Err(error) => return Err(error.into()),
        }
    } else if !path.exists() {
        return Ok(());
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

    #[test]
    fn test_forward_command_response_is_bounded() {
        let home_dir = temp_dir("home");
        let runtime_dir = temp_dir("runtime");
        let paths = RuntimePaths::from_roots(home_dir, runtime_dir, None);

        prepare_runtime(&paths).unwrap();
        let listener = bind_socket(&paths.socket_path).unwrap();
        let socket_path = paths.socket_path.clone();

        // Server sends more than MAX_COMMAND_BYTES
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = read_command(&mut stream).unwrap();
            let oversized = "x".repeat(MAX_COMMAND_BYTES + 1024);
            stream.write_all(oversized.as_bytes()).ok();
        });

        let response = forward_command(&socket_path, ControlCommand::GetStatus).unwrap();

        assert!(response.len() <= MAX_COMMAND_BYTES);
        handle.join().unwrap();
    }

    #[test]
    fn test_connection_guard_decrements_on_drop() {
        let counter = Arc::new(AtomicUsize::new(0));

        {
            let _guard = ConnectionGuard::acquire(&counter);
            assert_eq!(counter.load(Ordering::SeqCst), 1);

            {
                let _guard2 = ConnectionGuard::acquire(&counter);
                assert_eq!(counter.load(Ordering::SeqCst), 2);
            }
            // _guard2 dropped
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        }
        // _guard dropped
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
}
