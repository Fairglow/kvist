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

    /// A standalone ACP discovery omitted the required host-authority warning.
    #[error("ACP model discovery requires explicit --allow-host-discovery acknowledgement")]
    HostDiscoveryNotAcknowledged,

    /// A provider model catalog violates its canonical schema or protocol.
    #[error("invalid provider model catalog: {reason}")]
    ModelCatalogInvalid { reason: String },

    /// The overall provider model discovery deadline elapsed.
    #[error("provider model discovery timed out")]
    ModelDiscoveryTimedOut,

    /// A canonical model request violates a size, schema, or identity invariant.
    #[error("invalid model request: {reason}")]
    InvalidModelRequest { reason: String },

    /// A local model transport endpoint or limit is invalid.
    #[error("invalid model transport endpoint or limit: {reason}")]
    InvalidModelTransport { reason: String },

    /// The selected provider cannot preserve a requested capability.
    #[error("unsupported capability `{capability}` for provider `{provider}`")]
    UnsupportedCapability {
        provider: &'static str,
        capability: &'static str,
    },

    /// Cooperative cancellation stopped a model request.
    #[error("model transport was cancelled")]
    ModelTransportCancelled,

    /// The overall model request deadline elapsed.
    #[error("model transport timed out")]
    ModelTransportTimedOut,

    /// Slot allocation timeout elapsed before the local provider accepted the request.
    #[error("slot allocation timed out after {timeout:?}")]
    SlotAllocationTimedOut { timeout: std::time::Duration },

    /// Time-to-first-token watchdog elapsed before the local provider streamed initial tokens.
    #[error("time-to-first-token watchdog timed out after {timeout:?}")]
    TtftTimedOut { timeout: std::time::Duration },

    /// Inter-token cadence watchdog elapsed between streamed tokens.
    #[error("inter-token cadence watchdog timed out after {timeout:?}")]
    InterTokenCadenceTimedOut { timeout: std::time::Duration },

    /// The provider returned a non-success response; its body is intentionally omitted.
    #[error("model provider returned HTTP status {status}")]
    ModelProviderStatus { status: u16 },

    /// Provider response headers or body exceeded a hard bound.
    #[error("model provider exceeded the {max_bytes}-byte response limit")]
    ModelResponseLimitExceeded { max_bytes: usize },

    /// Provider HTTP or JSON did not satisfy the selected protocol.
    #[error("malformed model provider response: {reason}")]
    MalformedModelResponse { reason: String },

    /// One turn reused a tool call identity.
    #[error("duplicate tool call identity in model provider response")]
    DuplicateToolCall,

    /// A socket operation failed without retaining endpoint or payload data.
    #[error("model transport I/O failed while {operation}: {source}")]
    ModelTransportIo {
        operation: &'static str,
        #[source]
        source: io::Error,
    },

    /// A framework-backed request failed without exposing provider payload data.
    #[error("model transport framework failed while {operation}")]
    ModelTransportFramework { operation: &'static str },

    /// The supervised command exited unsuccessfully.
    #[error("supervised command failed with exit status: {status}")]
    ProcessFailed { status: ExitStatus },

    /// Every permitted attempt ended in the same retryable failure class.
    #[error("supervision stopped after {attempts} attempt(s): {reason}")]
    SupervisionExhausted { attempts: u32, reason: String },

    /// Combined stdout and stderr exceeded the configured bound.
    #[error("supervised command exceeded the {max_bytes}-byte output limit")]
    OutputLimitExceeded { max_bytes: usize },

    /// A command remained active beyond its total per-attempt deadline.
    #[error("supervised command exceeded its {max_milliseconds}-millisecond attempt timeout")]
    AttemptTimedOut { max_milliseconds: u128 },

    /// Output pipes remained open after the supervised process group ended.
    #[error("supervised process output remained open after process-group termination")]
    OutputStreamsRetained,

    /// SIGINT or SIGTERM requested cancellation of the complete process group.
    #[error("supervised command was cancelled")]
    Cancelled,

    /// A filesystem, process, or stream operation failed.
    #[error("cannot {operation}`{path}`: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl Error {
    /// Whether this failure is a transient, temporal condition that a bounded
    /// retry may recover from.
    ///
    /// Returns `true` for network drops, provider read/generation timeouts,
    /// transient slot-allocation stalls, and transient server errors (HTTP
    /// 429/5xx). Returns `false` for persistent failures that retry would not
    /// fix: cooperative cancellation, malformed responses, response-size limits,
    /// bad requests, and configuration errors.
    ///
    /// This lets a caller retry only the failures it can plausibly recover from,
    /// so a genuinely broken request is reported immediately instead of
    /// wasting retries.
    pub fn is_retryable(&self) -> bool {
        match self {
            // Temporal: the provider was slow, stalled, or the turn's deadline
            // elapsed while it was still generating. A fresh attempt gets a new
            // deadline and can finish.
            Error::ModelTransportTimedOut
            | Error::SlotAllocationTimedOut { .. }
            | Error::TtftTimedOut { .. }
            | Error::InterTokenCadenceTimedOut { .. } => true,
            // Network transport errors: resets, aborts, and timeouts are
            // transient; a protocol/encoding error is not.
            Error::ModelTransportIo { source, .. } => matches!(
                source.kind(),
                io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::TimedOut
                    | io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::UnexpectedEof
            ),
            // Transient server-side failures.
            Error::ModelProviderStatus { status } => {
                matches!(*status, 429 | 500 | 502 | 503 | 504 | 507)
            }
            // Everything else is treated as persistent: do not retry.
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn io_error(kind: io::ErrorKind) -> Error {
        Error::ModelTransportIo {
            operation: "reading",
            source: io::Error::new(kind, "boom"),
        }
    }

    #[test]
    fn temporal_failures_are_retryable() {
        assert!(Error::ModelTransportTimedOut.is_retryable());
        assert!(
            Error::SlotAllocationTimedOut {
                timeout: Duration::from_secs(1)
            }
            .is_retryable()
        );
        assert!(
            Error::TtftTimedOut {
                timeout: Duration::from_secs(1)
            }
            .is_retryable()
        );
        assert!(
            Error::InterTokenCadenceTimedOut {
                timeout: Duration::from_secs(1)
            }
            .is_retryable()
        );
        for kind in [
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::TimedOut,
            io::ErrorKind::ConnectionRefused,
            io::ErrorKind::UnexpectedEof,
        ] {
            assert!(
                io_error(kind).is_retryable(),
                "{kind:?} should be retryable"
            );
        }
        for status in [429, 500, 502, 503, 504, 507] {
            assert!(
                Error::ModelProviderStatus { status }.is_retryable(),
                "{status} should be retryable"
            );
        }
    }

    #[test]
    fn persistent_failures_are_not_retryable() {
        assert!(!Error::ModelTransportCancelled.is_retryable());
        assert!(
            !Error::InvalidModelRequest {
                reason: "too large".to_owned()
            }
            .is_retryable()
        );
        assert!(
            !Error::MalformedModelResponse {
                reason: "bad json".to_owned()
            }
            .is_retryable()
        );
        assert!(!Error::ModelResponseLimitExceeded { max_bytes: 1024 }.is_retryable());
        assert!(!Error::DuplicateToolCall.is_retryable());
        assert!(
            !Error::InvalidModelTransport {
                reason: "bad endpoint".to_owned()
            }
            .is_retryable()
        );
        // A non-transient socket error (bad input) is not retryable.
        assert!(!io_error(io::ErrorKind::InvalidInput).is_retryable());
        // A client error the server will not retry is not retryable.
        assert!(!Error::ModelProviderStatus { status: 422 }.is_retryable());
    }
}
