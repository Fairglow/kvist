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
    #[error("cannot {operation} `{path}`: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}
