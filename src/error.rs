use std::io::{self, Write};

use thiserror::Error;

/// Errors that Kvist can report independently of its presentation layer.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KvistError {
    /// A command-line syntax or help error produced by the parser.
    #[error(transparent)]
    ArgumentParsing(#[from] clap::Error),
    /// A command is part of the stable CLI contract but is not implemented in
    /// the current development phase.
    #[error("`{command}` is not available yet; {next_step}")]
    CommandUnavailable {
        /// Command name as shown to the user.
        command: &'static str,
        /// Specific next action that explains the command's status.
        next_step: &'static str,
    },
}

/// Result type used by Kvist's domain and command layers.
pub type Result<T> = std::result::Result<T, KvistError>;

impl KvistError {
    /// Returns the process exit status associated with this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::ArgumentParsing(error) => error.exit_code() as u8,
            Self::CommandUnavailable { .. } => 1,
        }
    }

    /// Writes this error using the parser's output policy when applicable.
    pub fn print(&self) -> io::Result<()> {
        match self {
            Self::ArgumentParsing(error) => error.print(),
            Self::CommandUnavailable { .. } => {
                let mut stderr = io::stderr().lock();
                writeln!(stderr, "error: {self}")
            }
        }
    }
}
