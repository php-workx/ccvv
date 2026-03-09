use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;

use ccvv_linux::tray::{
    icon_for_status, request_status, send_menu_action, watch_status_updates, TrayAction, TrayError,
    TrayIcon,
};
use ccvv_linux::ui_protocol::StatusSnapshot;
use clap::{Parser, Subcommand};
use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};

#[derive(Debug, Parser)]
#[command(name = "ccvv-tray-sni", about = "Socket-driven SNI tray sidecar")]
struct Cli {
    #[arg(long)]
    socket: PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Status,
    Pause,
    Resume,
    CleanNow,
    Quit,
}

#[derive(Clone, Debug)]
enum TrayEvent {
    Action(TrayAction),
    Status(StatusSnapshot),
    SocketClosed(Result<(), String>),
    WatcherOffline(String),
}

struct StatusNotifierTray {
    event_tx: mpsc::Sender<TrayEvent>,
    status: StatusSnapshot,
    last_error: Option<String>,
}

impl ksni::Tray for StatusNotifierTray {
    fn id(&self) -> String {
        "ccvv-tray-sni".to_string()
    }

    fn title(&self) -> String {
        let state = match icon_for_status(&self.status) {
            TrayIcon::Active => "active",
            TrayIcon::Paused => "paused",
            TrayIcon::Limited => "limited",
            TrayIcon::Error => "error",
        };
        format!("ccvv ({state})")
    }

    fn status(&self) -> ksni::Status {
        if self.last_error.is_some() || matches!(icon_for_status(&self.status), TrayIcon::Error) {
            return ksni::Status::NeedsAttention;
        }
        if self.status.paused {
            return ksni::Status::Passive;
        }

        ksni::Status::Active
    }

    fn icon_name(&self) -> String {
        match icon_for_status(&self.status) {
            TrayIcon::Active => "emblem-ok-symbolic".to_string(),
            TrayIcon::Paused => "media-playback-pause-symbolic".to_string(),
            TrayIcon::Limited => "dialog-warning-symbolic".to_string(),
            TrayIcon::Error => "dialog-error-symbolic".to_string(),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            standard_item("Clean Clipboard Now", Some(TrayAction::CleanNow), true),
            standard_item(
                if self.status.paused {
                    "Resume"
                } else {
                    "Pause"
                },
                Some(if self.status.paused {
                    TrayAction::Resume
                } else {
                    TrayAction::Pause
                }),
                true,
            ),
            StandardItem {
                label: format!("Backend: {:?}", self.status.backend),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.event_tx.send(TrayEvent::Action(TrayAction::Quit));
                }),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        let _ = self
            .event_tx
            .send(TrayEvent::WatcherOffline(format!("{reason:?}")));
        false
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        return run_command(&cli.socket, command);
    }

    match run_tray(cli.socket) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ccvv-tray-sni: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_command(socket_path: &Path, command: Command) -> ExitCode {
    let result = match command {
        Command::Status => request_status(socket_path).map(|status| {
            println!("{:?}", icon_for_status(&status));
        }),
        Command::Pause => send_menu_action(socket_path, TrayAction::Pause),
        Command::Resume => send_menu_action(socket_path, TrayAction::Resume),
        Command::CleanNow => send_menu_action(socket_path, TrayAction::CleanNow),
        Command::Quit => send_menu_action(socket_path, TrayAction::Quit),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ccvv-tray-sni: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_tray(socket_path: PathBuf) -> Result<(), TrayError> {
    let initial_status = request_status(&socket_path)?;
    let (event_tx, event_rx) = mpsc::channel();

    let tray = StatusNotifierTray {
        event_tx: event_tx.clone(),
        status: initial_status,
        last_error: None,
    };
    let handle = tray
        .assume_sni_available(true)
        .spawn()
        .map_err(|error| TrayError::Service(format!("failed to start tray service: {error:?}")))?;

    spawn_status_monitor(socket_path.clone(), event_tx.clone());

    while let Ok(event) = event_rx.recv() {
        match event {
            TrayEvent::Action(action) => {
                let result = send_menu_action(&socket_path, action);
                let last_error = result.err().map(|error| error.to_string());
                let should_exit = action == TrayAction::Quit && last_error.is_none();

                let _ = handle.update(|tray| {
                    tray.last_error = last_error.clone();
                });

                if should_exit {
                    break;
                }
            }
            TrayEvent::Status(status) => {
                let _ = handle.update(|tray| {
                    tray.status = status.clone();
                    tray.last_error = None;
                });
            }
            TrayEvent::SocketClosed(result) => {
                if let Err(error) = result {
                    let _ = handle.update(|tray| {
                        tray.last_error = Some(error);
                    });
                }
                break;
            }
            TrayEvent::WatcherOffline(reason) => {
                eprintln!("ccvv-tray-sni: tray watcher unavailable: {reason}");
                break;
            }
        }
    }

    handle.shutdown().wait();
    Ok(())
}

fn spawn_status_monitor(socket_path: PathBuf, event_tx: mpsc::Sender<TrayEvent>) {
    thread::spawn(move || {
        let (status_tx, status_rx) = mpsc::channel();
        let status_event_tx = event_tx.clone();
        let forwarder = thread::spawn(move || {
            while let Ok(status) = status_rx.recv() {
                if status_event_tx.send(TrayEvent::Status(status)).is_err() {
                    break;
                }
            }
        });

        let result =
            watch_status_updates(&socket_path, status_tx).map_err(|error| error.to_string());
        let _ = forwarder.join();
        let _ = event_tx.send(TrayEvent::SocketClosed(result));
    });
}

fn standard_item(
    label: &str,
    action: Option<TrayAction>,
    enabled: bool,
) -> MenuItem<StatusNotifierTray> {
    StandardItem {
        label: label.into(),
        enabled,
        activate: Box::new(move |tray: &mut StatusNotifierTray| {
            if let Some(action) = action {
                let _ = tray.event_tx.send(TrayEvent::Action(action));
            }
        }),
        ..Default::default()
    }
    .into()
}
