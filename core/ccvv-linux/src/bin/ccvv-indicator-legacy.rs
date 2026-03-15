use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ccvv_linux::tray::{
    icon_for_status, is_action_enabled, request_status, send_menu_action, title_for_status,
    watch_status_updates, TrayAction,
};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "ccvv-indicator-legacy",
    about = "Socket-driven legacy tray helper"
)]
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

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        return run_command(&cli.socket, command);
    }

    match run_helper(&cli.socket) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ccvv-indicator-legacy: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_command(socket_path: &Path, command: Command) -> ExitCode {
    let result = match command {
        Command::Status => request_status(socket_path).map(|status| {
            println!("{}", title_for_status(&status));
        }),
        Command::Pause => guarded_action(socket_path, TrayAction::Pause),
        Command::Resume => guarded_action(socket_path, TrayAction::Resume),
        Command::CleanNow => guarded_action(socket_path, TrayAction::CleanNow),
        Command::Quit => send_menu_action(socket_path, TrayAction::Quit),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ccvv-indicator-legacy: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_helper(socket_path: &Path) -> Result<(), ccvv_linux::tray::TrayError> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let socket_path = socket_path.to_path_buf();
    let monitor = std::thread::spawn(move || watch_status_updates(&socket_path, sender));

    while let Ok(status) = receiver.recv() {
        eprintln!(
            "ccvv-indicator-legacy: {:?} {}",
            icon_for_status(&status),
            title_for_status(&status)
        );
    }

    monitor.join().unwrap_or_else(|_| {
        Err(ccvv_linux::tray::TrayError::Service(
            "legacy monitor panicked".into(),
        ))
    })
}

fn guarded_action(
    socket_path: &Path,
    action: TrayAction,
) -> Result<(), ccvv_linux::tray::TrayError> {
    let status = request_status(socket_path)?;
    if !is_action_enabled(&status, action) {
        return Err(ccvv_linux::tray::TrayError::Service(format!(
            "{} is unavailable in the current backend mode",
            match action {
                TrayAction::Pause => "pause",
                TrayAction::Resume => "resume",
                TrayAction::CleanNow => "clean-now",
                TrayAction::Quit => "quit",
            }
        )));
    }

    send_menu_action(socket_path, action)
}
