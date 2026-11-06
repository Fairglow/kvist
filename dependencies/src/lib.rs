//! Locked dependency mechanics. Acquisition authority belongs to the caller.
#![forbid(unsafe_code)]

use std::{io, path::PathBuf};

mod fs;
mod integrity;
mod publication;
mod resolver;
pub mod rust;
pub mod vendoring;

/// Shared dependency failure domains, without captured subprocess payloads.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Dependency state is absent, unsafe, malformed or unsupported.
    #[error("vendoring unavailable for `{path}`: {reason}")]
    VendoringUnavailable { path: String, reason: String },
    /// Required packages are absent.
    #[error("vendoring incomplete for `{path}`: {count} missing ({missing})")]
    VendoringIncomplete {
        path: String,
        count: usize,
        missing: String,
    },
    /// The observed lock no longer matches.
    #[error("vendoring stale for `{path}`: {current}, expected {expected}")]
    VendoringStale {
        path: String,
        current: String,
        expected: String,
    },
    /// A filesystem operation failed.
    #[error("cannot {operation} `{path}`: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Cooperative cancellation stopped the operation.
    #[error("dependency operation cancelled")]
    Cancelled,
    /// Another operation owns the project lock; stale locks are never stolen.
    #[error("dependency operation lock occupied at `{path}`; inspect before reconciliation")]
    OperationLocked { path: PathBuf },
    /// A bounded Cargo process failed. No diagnostic payload is retained.
    #[error("Cargo acquisition failed: {failure:?}")]
    Process { failure: ProcessFailure },
    /// Multi-file publication failed, and retained material requires inspection.
    #[error(
        "dependency publication interrupted (published={published}); inspect `{recovery}`: {source}"
    )]
    Publication {
        published: bool,
        recovery: PathBuf,
        #[source]
        source: Box<Error>,
    },
}

/// Sanitized process outcomes preserved for frontend adaptation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessFailure {
    /// The program could not start or its streams failed.
    Io,
    /// Unsuccessful exit.
    Exit,
    /// Wall-clock or idle deadline expired.
    Timeout,
    /// Combined output exceeded its hard limit.
    OutputLimit,
    /// Descendants retained output streams.
    RetainedStreams,
    /// Unexpected supervisor failure.
    Supervision,
}

/// Shared result type.
pub type Result<T> = std::result::Result<T, Error>;

fn unavailable(path: &std::path::Path, reason: &'static str) -> Error {
    Error::VendoringUnavailable {
        path: path.to_string_lossy().into_owned(),
        reason: reason.into(),
    }
}
