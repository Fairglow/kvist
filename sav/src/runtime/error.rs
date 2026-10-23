//! Typed mechanism failures, separate from embedding application errors.

/// Failures that can occur at the reusable native runtime boundary.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Internal operation handoff; never whole-prompt cancellation.
    #[doc(hidden)]
    #[error("generation interrupted for guidance")]
    Steering { attempt: u32, reserve: u32 },
    /// Invalid runtime configuration.
    #[error("invalid configuration: {reason}")]
    Config {
        path: Option<String>,
        reason: String,
    },
    /// Model I/O or protocol failure.
    #[error("model request failed: {reason}")]
    ModelTransport {
        model: Option<String>,
        reason: String,
    },
    /// A request and its generation reserve do not fit.
    #[error(
        "request needs {used} input tokens plus {reserve} output tokens; context limit is {limit}"
    )]
    ContextBudget {
        used: usize,
        limit: usize,
        reserve: usize,
    },
    /// An incomplete or inconsistent proposal.
    #[error("invalid model turn: {reason}")]
    InvalidModelTurn { reason: String },
    /// Shared prompt resource exhaustion.
    #[error("prompt budget exhausted: {reason}")]
    RunBudget { reason: String },
    /// Required recording failure.
    #[error("runtime recording failed: {reason}")]
    Recording { reason: String },
    /// Host refused a tool before effects.
    #[error("tool `{tool}` is not permitted: {reason}")]
    ToolPolicy { tool: String, reason: String },
    /// Host could not render a tool before effects.
    #[error("could not render tool `{tool}`: {reason}")]
    ToolRender { tool: String, reason: String },
    /// Other terminal host execution failure; effects may have occurred.
    #[error("host execution failed: {reason}")]
    Host { reason: String },
    /// Worker event destination is closed.
    #[error("the runtime event connection closed")]
    ChannelClosed,
    /// Invalid, full, closed or no-longer-editable controls.
    #[error("runtime control rejected: {reason}")]
    Control { reason: String },
    /// Failure to create or manage owned runtime resources.
    #[error("runtime I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Runtime result, independent of application error types.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Bounded printable diagnostics suitable for operational events.
    pub fn describe(&self) -> String {
        self.to_string()
            .chars()
            .take(4096)
            .flat_map(|ch| {
                if ch.is_control() && ch != '\n' && ch != '\t' {
                    ch.escape_default().collect::<Vec<_>>()
                } else {
                    vec![ch]
                }
            })
            .collect()
    }
}

impl From<crate::Error> for Error {
    fn from(error: crate::Error) -> Self {
        Self::ModelTransport {
            model: None,
            reason: error.to_string(),
        }
    }
}
