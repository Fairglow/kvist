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
pub mod cli;
pub mod component_documents;
pub mod config;
pub mod convert;
pub mod discovery;
mod error;
mod file_io;
mod filesystem;
pub mod import;
pub mod init;
pub mod logging;
pub mod project_state;
pub mod prompt_input;
pub mod reverse_discovery;
pub mod sandbox;
pub mod shell;
pub mod status;
pub mod task_commands;
pub mod task_queue;
pub mod tree;
pub mod vcs;
pub mod vcs_commit;
pub mod wizard;

use clap::Parser;

pub use error::{KvistError, Result};
pub use logging::{init_logging, init_test_logging};

/// Parses process arguments and dispatches the requested command.
pub fn run() -> Result<cli::CommandOutput> {
    logging::init_logging();
    let cli = cli::Cli::try_parse()?;
    cli::execute(cli.command, cli.json)
}
