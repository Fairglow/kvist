//! Blad: first-class Markdown-native transcripts for agent sessions.
//!
//! Each session produces a durable directory:
//! ```
//! .blad/sessions/{session-id}/
//! ├── manifest.json
//! └── messages/
//!     ├── 000001.md
//!     └── 000002.md
//! ```

use clap::Parser;

use crate::cli::{Cli, Command};

mod cli;
mod continuation;
mod display;
mod export;
mod interactive;
mod listing;
mod session_manager;
mod transcript_writer;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::New {
            model,
            effort,
            cwd,
            no_logs,
        } => {
            interactive::run_new_session(model, effort, cwd, no_logs);
        }
        Command::Continue {
            session_id,
            model,
            effort,
        } => {
            interactive::run_continue_session(&session_id, model, effort);
        }
        Command::List { limit } => {
            listing::run_list_sessions(limit);
        }
        Command::Show {
            session_id,
            message,
        } => {
            display::run_show_session(&session_id, message);
        }
        Command::Export { session_id, output } => {
            export::run_export_session(&session_id, output);
        }
    }
}
