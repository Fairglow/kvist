//! Command-line interface for Blad.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[clap(
    name = "blad",
    about = "First-class Markdown-native transcripts for agent sessions"
)]
pub struct Cli {
    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start a new interactive agent session
    New {
        /// Model identifier to use
        #[clap(long)]
        model: Option<String>,

        /// Thinking effort level (low, medium, high)
        #[clap(long)]
        effort: Option<String>,

        /// Working directory for the session
        #[clap(long, default_value = ".")]
        cwd: PathBuf,

        /// Skip writing durable artifacts
        #[clap(long)]
        no_logs: bool,
    },

    /// Continue a previous session as a new session
    Continue {
        /// Session ID to continue
        session_id: String,

        /// Model identifier to use
        #[clap(long)]
        model: Option<String>,

        /// Thinking effort level (low, medium, high)
        #[clap(long)]
        effort: Option<String>,
    },

    /// List past sessions
    List {
        /// Maximum number of sessions to show
        #[clap(long, default_value = "20")]
        limit: usize,
    },

    /// Show a specific session's transcript
    Show {
        /// Session ID to show
        session_id: String,

        /// Show only a specific message by sequence number
        #[clap(long)]
        message: Option<usize>,
    },

    /// Export a session as a combined Markdown document
    Export {
        /// Session ID to export
        session_id: String,

        /// Output file path (default: stdout)
        #[clap(long)]
        output: Option<PathBuf>,
    },
}
