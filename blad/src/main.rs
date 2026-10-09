//! Blad: Skott session viewer and transcript browser.
//!
//! Reads skott's structured session transcripts from .skott/runs/ and displays
//! them with full markdown rendering. Can also view standalone .md files.
//!
//! Usage:
//!   blad list                     - list recent sessions
//!   blad show <session-id>        - view a completed session
//!   blad stream <session-id>      - follow a live session
//!   blad view <file.md>           - view any markdown file
//!   blad export <session-id>      - export session to combined markdown
//!   blad <session-id|file.md>     - smart shortcut (show or view)

use clap::Parser;
use std::path::PathBuf;

mod display;
mod session;

const DEFAULT_LOG_DIR: &str = ".skott/runs";

#[derive(Parser)]
#[clap(name = "blad", about = "Skott session viewer and markdown browser")]
pub struct Cli {
    /// Directory containing skott session runs (default: .skott/runs)
    #[clap(long, global = true, value_name = "PATH")]
    log_dir: Option<PathBuf>,

    /// Session ID or file path (smart shortcut for show/view)
    target: Option<String>,

    #[clap(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// List recent skott sessions
    List {
        #[clap(long, default_value = "20")]
        limit: usize,
    },
    /// View a completed session transcript
    Show { session_id: String },
    /// Follow a live session as it streams
    Stream { session_id: String },
    /// View a standalone markdown file
    View { path: PathBuf },
    /// Export a session to a combined markdown document
    Export {
        session_id: String,
        #[clap(long)]
        output: Option<PathBuf>,
    },
}

fn resolve_log_dir(cli: &Cli) -> PathBuf {
    match &cli.log_dir {
        Some(path) => path.clone(),
        None => PathBuf::from(DEFAULT_LOG_DIR),
    }
}

fn main() {
    let cli = Cli::parse();
    let log_dir = resolve_log_dir(&cli);

    // Smart shortcut: if no subcommand but a target was given, try show then view
    if cli.command.is_none() {
        if let Some(target) = &cli.target {
            if target.ends_with(".md") || target.ends_with(".markdown") {
                // Treat as a file path
                match std::fs::read_to_string(target) {
                    Ok(content) => display::view_markdown(&content),
                    Err(e) => eprintln!("Error reading file: {}", e),
                }
                return;
            }

            // Try as a session ID
            match session::load_session(&log_dir, target) {
                Some(s) => display::show_session(&s),
                None => {
                    eprintln!("Session not found: {}", target);
                }
            }
        } else {
            eprintln!("Usage: blad [OPTIONS] <command> [ARGS]");
        }
        return;
    }

    match cli.command {
        Some(Command::List { limit }) => {
            let sessions = session::list_sessions(&log_dir, limit);
            if sessions.is_empty() {
                println!("No sessions found in {}", log_dir.display());
                return;
            }
            for s in &sessions {
                let model = s.model.as_deref().unwrap_or("unknown");
                let status = if s.completed { "completed" } else { "active" };
                println!(
                    "{} - {} - {} messages - {}",
                    s.id, model, s.message_count, status
                );
            }
        }
        Some(Command::Show { session_id }) => match session::load_session(&log_dir, &session_id) {
            Some(s) => display::show_session(&s),
            None => {
                eprintln!("Session not found: {}", session_id);
            }
        },
        Some(Command::Stream { session_id }) => {
            match session::load_session(&log_dir, &session_id) {
                Some(s) => display::stream_session(&s),
                None => {
                    eprintln!("Session not found: {}", session_id);
                }
            }
        }
        Some(Command::View { path }) => match std::fs::read_to_string(&path) {
            Ok(content) => display::view_markdown(&content),
            Err(e) => {
                eprintln!("Error reading file: {}", e);
            }
        },
        Some(Command::Export { session_id, output }) => {
            match session::load_session(&log_dir, &session_id) {
                Some(s) => display::export_session(&s, output.as_deref()),
                None => {
                    eprintln!("Session not found: {}", session_id);
                }
            }
        }
        None => {}
    }
}
