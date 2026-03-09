//! ccvv CLI companion.
//!
//! Provides stdin/stdout pipe transformation, history access,
//! diagnostics, and config validation.
//! See §10 of the technical spec.

use std::io::{self, Read, Write};

use clap::{Parser, Subcommand};
use similar::{ChangeTag, TextDiff};

use ccvv_lib::config::{load_config, resolve_config, validate_config};
use ccvv_lib::pipeline::Pipeline;
use ccvv_lib::transforms::normalize::NormalizeTransform;
use ccvv_lib::transforms::structural::StructuralTransform;
use ccvv_lib::transforms::url::UrlTransform;
use ccvv_lib::transforms::whitespace::WhitespaceTransform;
use ccvv_lib::transforms::Transform;

#[derive(Parser)]
#[command(name = "ccvv", version = "1.1.0", about = "Clipboard text sanitizer")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Config file path (default: ~/.ccvv/config.toml)
    #[arg(long, global = true)]
    config: Option<String>,

    /// Config profile to use
    #[arg(long, global = true)]
    profile: Option<String>,

    /// Only strip URLs (run URL cleaning stage)
    #[arg(long)]
    strip_urls: bool,

    /// Flatten JSON (compact single line)
    #[arg(long)]
    flatten_json: bool,

    /// Prettify JSON
    #[arg(long)]
    prettify_json: bool,

    /// Unwrap text (remove line breaks within paragraphs)
    #[arg(long)]
    unwrap: bool,

    /// Normalize unicode characters
    #[arg(long)]
    normalize: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Show unified diff of what would change
    Preview,

    /// Access clipboard history
    History {
        /// Search query
        #[arg(long)]
        search: Option<String>,

        /// Filter by content type
        #[arg(long, name = "type")]
        content_type: Option<String>,

        /// Number of entries to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// System diagnostics
    Doctor,

    /// Validate config file
    Validate,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Preview) => cmd_preview(&cli),
        Some(Commands::History {
            search,
            content_type,
            limit,
        }) => cmd_history(search, content_type, limit),
        Some(Commands::Doctor) => cmd_doctor(&cli),
        Some(Commands::Validate) => cmd_validate(&cli),
        None => cmd_transform(&cli),
    }
}

/// Default: read stdin, transform, write stdout.
fn cmd_transform(cli: &Cli) {
    let input = read_stdin();
    let pipeline = build_pipeline(cli);
    let (result, ctx) = pipeline.run(&input);

    if ctx.skipped_sensitive {
        eprintln!("ccvv: input contains sensitive content, skipped");
    }
    if ctx.skipped_oversize {
        eprintln!("ccvv: input too large, skipped");
    }

    io::stdout().write_all(result.as_bytes()).unwrap();
}

/// Preview: show unified diff.
fn cmd_preview(cli: &Cli) {
    let input = read_stdin();
    let pipeline = build_pipeline(cli);
    let (result, _ctx) = pipeline.run(&input);

    if input == result {
        eprintln!("No changes.");
        return;
    }

    let diff = TextDiff::from_lines(&input, &result);

    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        print!("{}{}", sign, change);
    }
}

/// History: list, search, or filter entries.
fn cmd_history(search: Option<String>, content_type: Option<String>, limit: usize) {
    let home = std::env::var("HOME").unwrap_or_default();
    let db_path = std::path::PathBuf::from(&home)
        .join(".ccvv")
        .join("history.db");

    if !db_path.exists() {
        eprintln!("No history database found at {:?}", db_path);
        std::process::exit(1);
    }

    let db = match ccvv_lib::history::HistoryDb::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("Failed to open history: {}", e);
            std::process::exit(1);
        }
    };

    let entries = if let Some(q) = search {
        db.search(&q, limit).unwrap_or_default()
    } else if let Some(ct) = content_type {
        db.by_type(&ct, limit).unwrap_or_default()
    } else {
        db.recent(limit).unwrap_or_default()
    };

    if entries.is_empty() {
        println!("No history entries found.");
        return;
    }

    for entry in &entries {
        println!(
            "[{}] {} | {} | {}",
            entry.id,
            entry.created_at,
            entry.content_type.as_deref().unwrap_or("unknown"),
            entry.preview
        );
    }
}

/// Doctor: system diagnostics.
fn cmd_doctor(cli: &Cli) {
    println!("ccvv doctor v1.1.0");
    println!();

    // Platform info
    println!("Platform: {}", std::env::consts::OS);
    println!("Architecture: {}", std::env::consts::ARCH);
    println!();

    // Config info
    let config_path = cli
        .config
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            std::path::PathBuf::from(&home)
                .join(".ccvv")
                .join("config.toml")
        });
    println!("Config path: {:?}", config_path);
    println!(
        "Config exists: {}",
        if config_path.exists() { "yes" } else { "no" }
    );

    if config_path.exists() {
        match load_config(Some(&config_path)) {
            Ok(config) => {
                let errors = validate_config(&config);
                if errors.is_empty() {
                    println!("Config valid: yes");
                } else {
                    println!("Config valid: no");
                    for e in &errors {
                        println!("  - {}", e);
                    }
                }
            }
            Err(e) => println!("Config load error: {}", e),
        }
    }
    println!();

    // Feature states
    let config = load_config(
        cli.config
            .as_ref()
            .map(|p| std::path::Path::new(p.as_str())),
    )
    .unwrap_or_default();
    let resolved = resolve_config(&config, cli.profile.as_deref()).unwrap_or_default();

    println!("Feature states:");
    println!(
        "  normalize_unicode: {}",
        if resolved.settings.normalize_unicode {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  whitespace_cleanup: {}",
        if resolved.settings.whitespace_cleanup {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  agent_strip: {}",
        if resolved.settings.agent_strip {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  structural_detection: {}",
        if resolved.settings.structural_detection {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  url_cleaning: {}",
        if resolved.settings.url_cleaning {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  auto_wrapper: {}",
        if resolved.settings.auto_wrapper {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  sensitive_filter: {}",
        if resolved.settings.sensitive_filter {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!();

    // History DB info
    let home = std::env::var("HOME").unwrap_or_default();
    let db_path = std::path::PathBuf::from(&home)
        .join(".ccvv")
        .join("history.db");
    println!("History DB path: {:?}", db_path);
    println!(
        "History DB exists: {}",
        if db_path.exists() { "yes" } else { "no" }
    );
    if db_path.exists() {
        if let Ok(meta) = std::fs::metadata(&db_path) {
            println!("History DB size: {} bytes", meta.len());
        }
    }

    println!();
    println!("WARNING: ccvv never makes network connections.");
    if !resolved.settings.sensitive_filter {
        println!(
            "WARNING: Sensitive content filter is DISABLED. Secret content may be transformed."
        );
    }
}

/// Validate: check config file.
fn cmd_validate(cli: &Cli) {
    let config_path = cli
        .config
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            std::path::PathBuf::from(&home)
                .join(".ccvv")
                .join("config.toml")
        });

    if !config_path.exists() {
        println!("Config file not found: {:?}", config_path);
        println!("Using default configuration.");
        return;
    }

    match load_config(Some(&config_path)) {
        Ok(config) => {
            let errors = validate_config(&config);
            if errors.is_empty() {
                println!("Config is valid.");

                // Also try resolving
                match resolve_config(&config, cli.profile.as_deref()) {
                    Ok(_) => println!("Config resolves successfully."),
                    Err(e) => {
                        println!("Config resolution error: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                println!("Config has {} error(s):", errors.len());
                for e in &errors {
                    println!("  - {}", e);
                }
                std::process::exit(1);
            }
        }
        Err(e) => {
            println!("Config load error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Read all of stdin.
fn read_stdin() -> String {
    let mut input = String::new();
    match io::stdin().read_to_string(&mut input) {
        Ok(_) => input,
        Err(e) => {
            eprintln!("ccvv: failed to read stdin: {}", e);
            std::process::exit(1);
        }
    }
}

/// Build a pipeline based on CLI flags.
fn build_pipeline(cli: &Cli) -> Pipeline {
    let config = load_config(
        cli.config
            .as_ref()
            .map(|p| std::path::Path::new(p.as_str())),
    )
    .unwrap_or_default();
    let resolved = resolve_config(&config, cli.profile.as_deref()).unwrap_or_default();

    let selective =
        cli.strip_urls || cli.normalize || cli.unwrap || cli.prettify_json || cli.flatten_json;
    let stages = if selective {
        build_selective_stages(cli)
    } else {
        return Pipeline::from_resolved_config(&resolved);
    };

    Pipeline::new(stages)
        .with_max_input_bytes(resolved.settings.max_input_bytes)
        .with_sensitive_filter(resolved.settings.sensitive_filter)
}

fn build_selective_stages(cli: &Cli) -> Vec<Box<dyn Transform>> {
    let mut stages: Vec<Box<dyn Transform>> = Vec::new();
    if cli.normalize {
        stages.push(Box::new(NormalizeTransform::new()));
    }
    if cli.unwrap {
        stages.push(Box::new(WhitespaceTransform::new()));
    }
    if cli.prettify_json || cli.flatten_json {
        stages.push(Box::new(StructuralTransform::new()));
    }
    if cli.strip_urls {
        stages.push(Box::new(UrlTransform::new()));
    }
    stages
}
