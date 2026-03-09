use std::path::PathBuf;
use std::process::ExitCode;

use ccvv_linux::app;
use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum BackendOverride {
    Auto,
    None,
}

#[derive(Debug, Parser)]
#[command(
    name = "ccvv-linux",
    version,
    about = "Headless Linux session daemon for ccvv"
)]
struct Cli {
    /// Backend override used during daemon startup.
    #[arg(long, value_enum, default_value_t = BackendOverride::Auto)]
    backend: BackendOverride,

    /// Config file path (default: ~/.ccvv/config.toml).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Config profile to use.
    #[arg(long)]
    profile: Option<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let options = app::AppOptions {
        backend: match cli.backend {
            BackendOverride::Auto => app::BackendOverride::Auto,
            BackendOverride::None => app::BackendOverride::None,
        },
        config_path: cli.config,
        profile: cli.profile,
    };

    match app::run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ccvv-linux: {error}");
            ExitCode::FAILURE
        }
    }
}
