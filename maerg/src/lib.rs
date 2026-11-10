#![forbid(unsafe_code)]

//! The Kvist command-line interface.
//!
//! The binary owns process exit codes and standard-error output. This crate
//! owns command parsing, dispatch, and domain errors so those behaviors remain
//! testable without spawning a process.

#[cfg(not(target_os = "linux"))]
compile_error!("Kvist currently supports Linux only");

pub mod acquisition;
pub mod agent;
pub mod artifacts;
pub mod authoring;
pub mod bark;
pub mod cli;
pub mod component_documents;
pub mod config;
pub mod context;
pub mod convert;
pub mod discovery;
mod error;
pub(crate) mod file_io;
mod filesystem;
pub mod help;
pub mod import;
pub mod init;
pub(crate) mod interruptible_stdin;
pub mod language_vendoring;
pub mod language_verification;
pub mod logging;
mod markdown_structure;
pub mod model_settings;
pub mod presentation;
pub mod project_state;
pub mod prompt_input;
pub mod repair;
pub mod reverse_discovery;
pub mod sandbox;
pub mod status;
mod status_markdown;
pub mod task_commands;
pub mod task_queue;
pub mod toolchain;
pub mod tree;
pub mod vcs;
pub mod vcs_commit;
pub mod vendor_command;
pub mod vendoring;
pub mod wizard;
pub mod skott_integration;

use clap::Parser;

pub use error::{KvistError, Result};
pub use logging::{init_logging, init_test_logging};

/// Parses process arguments and dispatches the requested command.
pub fn run() -> Result<cli::CommandOutput> {
    logging::init_logging();
    let cli = cli::Cli::try_parse()?;
    
    // Skott is now mandatory for Kvist agent execution
    // Launch Skott session automatically for agent/task commands
    if matches!(cli.command, Some(crate::cli::Command::Agent { .. }) | Some(crate::cli::Command::Task { .. })) {
        // Try to launch Skott in the background
        // If Skott fails to launch, we log a warning but continue
        let _ = skott_integration::SkottSession::launch(
            "kvist-agent",
            None // model determined by Kvist
        );
    }
    
    cli::execute(cli.command, cli.json)
}
