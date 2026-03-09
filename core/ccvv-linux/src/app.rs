use std::path::PathBuf;

use thiserror::Error;

use ccvv_lib::config::{load_config, resolve_config};
use ccvv_lib::history::HistoryDb;
use ccvv_lib::CcvvError;

use crate::control::socket::{enforce_private_file_mode, prepare_runtime, RuntimePaths};
use crate::single_instance::{acquire_single_instance, InstanceGuard};
use crate::ui_protocol::ControlCommand;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendOverride {
    Auto,
    None,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeBackend {
    None,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppOptions {
    pub backend: BackendOverride,
    pub config_path: Option<PathBuf>,
    pub profile: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct Bootstrap {
    pub paths: RuntimePaths,
    pub warnings: Vec<String>,
    pub singleton: InstanceGuard,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] CcvvError),
    #[error(transparent)]
    Runtime(#[from] crate::control::socket::RuntimeError),
    #[error(transparent)]
    SingleInstance(#[from] crate::single_instance::SingleInstanceError),
}

pub fn run(options: AppOptions) -> Result<(), AppError> {
    let config = load_config(options.config_path.as_deref())?;
    let _resolved = resolve_config(&config, options.profile.as_deref())?;
    let bootstrap = bootstrap(&options)?;

    if matches!(&bootstrap.singleton, InstanceGuard::Forwarded) {
        eprintln!("ccvv-linux: forwarded command to existing daemon");
        return Ok(());
    }

    let backend = select_backend(&options);

    match backend {
        RuntimeBackend::None => {
            for warning in bootstrap.warnings {
                eprintln!("ccvv-linux: warning: {warning}");
            }
            eprintln!("ccvv-linux: starting in none mode");
            Ok(())
        }
    }
}

fn bootstrap(options: &AppOptions) -> Result<Bootstrap, AppError> {
    let paths = RuntimePaths::from_env(options.config_path.clone())?;
    let mut warnings = prepare_runtime(&paths)?;
    let _history = HistoryDb::open(&paths.history_path)?;
    enforce_private_file_mode(&paths.history_path, &mut warnings)?;
    let singleton = acquire_single_instance(&paths.socket_path, ControlCommand::GetStatus)?;

    Ok(Bootstrap {
        paths,
        warnings,
        singleton,
    })
}

fn select_backend(options: &AppOptions) -> RuntimeBackend {
    match options.backend {
        BackendOverride::None => RuntimeBackend::None,
        BackendOverride::Auto => {
            if std::env::var_os("WAYLAND_DISPLAY").is_some()
                || std::env::var_os("DISPLAY").is_some()
            {
                eprintln!(
                    "ccvv-linux: display backend detection is scaffolded only; falling back to none mode"
                );
            }

            RuntimeBackend::None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    use super::{bootstrap, AppOptions, BackendOverride};

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

    fn restore_env_var(key: &str, value: Option<std::ffi::OsString>) {
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
}
