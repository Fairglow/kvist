use std::{io, path::PathBuf, process::ExitStatus};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// Failures reported by prompt acquisition, rendering, and supervision.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// A command template is empty, malformed, or contains an unusable path.
    #[error("invalid command template: {reason}")]
    InvalidCommandTemplate { reason: String },

    /// A prompt source did not produce bounded nonblank UTF-8.
    #[error("invalid prompt input: {reason}")]
    InvalidPromptInput { reason: String },

    /// A supervision limit is zero or exceeds its hard maximum.
    #[error("invalid supervision policy: {reason}")]
    InvalidPolicy { reason: String },

    /// A provider profile or interactive setup value is invalid.
    #[error("model profile setup failed: {reason}")]
    ProfileSetup { reason: String },

    /// A profile configuration file is malformed or unsupported.
    #[error("invalid profile configuration `{path}`: {reason}")]
    InvalidProfileConfiguration { path: PathBuf, reason: String },

    /// A requested name is absent from the selected profile store.
    #[error("profile `{name}` does not exist in `{path}`")]
    ProfileNotFound { name: String, path: PathBuf },

    /// A standalone invocation omitted the required host-authority warning.
    #[error("host execution requires explicit --allow-host-execution acknowledgement")]
    HostExecutionNotAcknowledged,

    /// The supervised command exited unsuccessfully.
    #[error("supervised command failed with exit status: {status}")]
    ProcessFailed { status: ExitStatus },

    /// Every permitted attempt ended in the same retryable failure class.
    #[error("supervision stopped after {attempts} attempt(s): {reason}")]
    SupervisionExhausted { attempts: u32, reason: String },

    /// Combined stdout and stderr exceeded the configured bound.
    #[error("supervised command exceeded the {max_bytes}-byte output limit")]
    OutputLimitExceeded { max_bytes: usize },

    /// Output pipes remained open after the supervised process group ended.
    #[error("supervised process output remained open after process-group termination")]
    OutputStreamsRetained,

    /// SIGINT or SIGTERM requested cancellation of the complete process group.
    #[error("supervised command was cancelled")]
    Cancelled,

    /// A filesystem, process, or stream operation failed.
    #[error("cannot {operation} `{path}`: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}
