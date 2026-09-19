//! Crate error type with actionable, non-secret messages and exit codes.

use std::fmt;

/// The single error type for the agent-runner crate.
///
/// Every variant carries enough context to act on the failure and an exit code.
/// Messages never expose secrets (tokens, credentials, or raw untrusted tool
/// output); they name the resource, the operation, and a remediation hint.
#[derive(Debug)]
pub enum Error {
    /// Configuration could not be read or failed validation.
    Config {
        /// The configuration path, when known.
        path: Option<String>,
        /// A human-readable reason.
        reason: String,
    },
    /// Standard input is not an interactive terminal.
    NotInteractive {
        /// Why interactive input is unavailable.
        reason: String,
    },
    /// The selected model id is not present in the configuration.
    ModelNotFound {
        /// The requested model id.
        requested: String,
        /// The configured model ids.
        available: Vec<String>,
    },
    /// The reasoning effort string did not parse to a known level.
    InvalidEffort {
        /// The offending string.
        value: String,
    },
    /// A path was missing, not a directory, or otherwise invalid.
    InvalidPath {
        /// The offending path.
        path: String,
        /// Why it is invalid.
        reason: String,
    },
    /// The sandbox runner or backend is unavailable or unverified.
    SandboxUnavailable {
        /// The configured runner path, when known.
        runner: Option<String>,
        /// Why the boundary could not be established.
        reason: String,
    },
    /// The sandbox request could not be constructed.
    SandboxBuild {
        /// Why construction failed.
        reason: String,
    },
    /// A sandbox execution failed.
    SandboxExec {
        /// The failing tool name, when known.
        tool: Option<String>,
        /// The root cause message.
        reason: String,
    },
    /// A tool call was rejected by policy before any sandbox request.
    ToolPolicy {
        /// The tool that was rejected.
        tool: String,
        /// Why the call violates policy.
        reason: String,
    },
    /// A tool argument was malformed or could not be rendered.
    ToolRender {
        /// The tool that could not be rendered.
        tool: String,
        /// The reason it could not be rendered.
        reason: String,
    },
    /// The model transport failed.
    ModelTransport {
        /// The provider or model selector, when known.
        model: Option<String>,
        /// The root cause message.
        reason: String,
    },
    /// A generic input/output failure.
    Io {
        /// The operation in progress.
        operation: String,
        /// The offending path, when relevant.
        path: Option<String>,
        /// The underlying source.
        source: std::io::Error,
    },
    /// The bounded event channel between the worker and the UI closed early.
    ChannelClosed,
}

/// The convenience `Result` alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

fn format_source(source: &std::io::Error) -> String {
    let message = source.to_string();
    let truncated: String = message.chars().take(200).collect();
    if truncated.len() == message.len() {
        truncated
    } else {
        format!("{truncated}… (truncated)")
    }
}

impl Error {
    /// The process exit code to use for this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::NotInteractive { .. } => 2,
            Error::ModelNotFound { .. } => 2,
            Error::InvalidEffort { .. } => 2,
            Error::ToolPolicy { .. } => 3,
            _ => 1,
        }
    }

    /// An actionable, non-secret message describing the failure.
    pub fn describe(&self) -> String {
        match self {
            Error::Config { path, reason } => match path {
                Some(path) => format!("invalid configuration `{path}`: {reason}"),
                None => format!("invalid configuration: {reason}"),
            },
            Error::NotInteractive { reason } => {
                format!("interactive input unavailable: {reason}")
            }
            Error::ModelNotFound {
                requested,
                available,
            } => format!(
                "unknown model `{requested}`; configured models: {}",
                if available.is_empty() {
                    "none".to_owned()
                } else {
                    available.join(", ")
                }
            ),
            Error::InvalidEffort { value } => format!(
                "invalid thinking effort `{value}`; expected none, minimal, low, medium, high, xhigh, or max"
            ),
            Error::InvalidPath { path, reason } => format!("invalid path `{path}`: {reason}"),
            Error::SandboxUnavailable { runner, reason } => match runner {
                Some(runner) => format!(
                    "sandbox unavailable: {reason} (check `sandbox.runner` and `sandbox.backend` for `{runner}`)"
                ),
                None => format!("sandbox unavailable: {reason}"),
            },
            Error::SandboxBuild { reason } => {
                format!("could not build the sandbox request: {reason}")
            }
            Error::SandboxExec { tool, reason } => match tool {
                Some(tool) => format!("sandbox execution of `{tool}` failed: {reason}"),
                None => format!("sandbox execution failed: {reason}"),
            },
            Error::ToolPolicy { tool, reason } => {
                format!("tool `{tool}` is not permitted: {reason}")
            }
            Error::ToolRender { tool, reason } => {
                format!("could not render tool `{tool}`: {reason}")
            }
            Error::ModelTransport { model, reason } => match model {
                Some(model) => format!("model `{model}` request failed: {reason}"),
                None => format!("model request failed: {reason}"),
            },
            Error::Io {
                operation,
                path,
                source,
            } => match path {
                Some(path) => format!("{operation} `{path}` failed: {}", format_source(source)),
                None => format!("{operation} failed: {}", format_source(source)),
            },
            Error::ChannelClosed => {
                "the worker connection closed before the turn finished".to_owned()
            }
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.describe())
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Error::Io {
            operation: "operation".to_owned(),
            path: None,
            source,
        }
    }
}

/// Builds a generic input/output error with an explicit operation and path.
pub fn io_error(operation: &str, path: Option<&str>, source: std::io::Error) -> Error {
    Error::Io {
        operation: operation.to_owned(),
        path: path.map(str::to_owned),
        source,
    }
}

impl From<agent_runtime::Error> for Error {
    fn from(source: agent_runtime::Error) -> Self {
        Error::ModelTransport {
            model: None,
            reason: source.to_string(),
        }
    }
}

impl From<kvist_sandbox_runner::validation::ProtocolError> for Error {
    fn from(source: kvist_sandbox_runner::validation::ProtocolError) -> Self {
        Error::SandboxBuild {
            reason: source.to_string(),
        }
    }
}
