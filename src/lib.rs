#![forbid(unsafe_code)]

//! The Kvist command-line interface.
//!
//! The binary owns process exit codes and standard-error output. This crate
//! owns command parsing, dispatch, and domain errors so those behaviors remain
//! testable without spawning a process.

pub mod cli;
mod error;

use clap::Parser;

pub use error::{KvistError, Result};

/// Parses process arguments and dispatches the requested command.
pub fn run() -> Result<()> {
    let cli = cli::Cli::try_parse()?;
    cli::execute(cli.command)
}
