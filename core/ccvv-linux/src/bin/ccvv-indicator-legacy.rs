use std::path::PathBuf;
use std::process::ExitCode;

use ccvv_linux::tray::{icon_for_status, request_status, send_menu_action, TrayAction};
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
    command: Command,
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
    let result = match cli.command {
        Command::Status => request_status(&cli.socket).map(|status| {
            println!("{:?}", icon_for_status(&status));
        }),
        Command::Pause => send_menu_action(&cli.socket, TrayAction::Pause),
        Command::Resume => send_menu_action(&cli.socket, TrayAction::Resume),
        Command::CleanNow => send_menu_action(&cli.socket, TrayAction::CleanNow),
        Command::Quit => send_menu_action(&cli.socket, TrayAction::Quit),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ccvv-indicator-legacy: {error}");
            ExitCode::FAILURE
        }
    }
}
