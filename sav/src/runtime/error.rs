//! Typed mechanism failures, separate from embedding application errors.

/// Failures that can occur at the reusable native runtime boundary.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An explicit cancellation, not a transport or resource guess.
    #[error("prompt cancelled")]
    Cancelled,
    /// Internal operation handoff; never whole-prompt cancellation.
    #[doc(hidden)]
    #[error("generation interrupted for guidance")]
    Steering { attempt: u32, reserve: u32 },
    /// A repetitive provider proposal was discarded before effects.
    #[error("repetitive generation detected: {report}")]
    RepetitiveGeneration {
        report: crate::loop_detection::RepetitionReport,
        usage: Option<crate::ModelUsage>,
    },
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
    /// A definitive provider context rejection, with no replay implied.
    #[error(
        "provider serving context exceeded; use a larger serving window or reduce irreducible task input"
    )]
    ModelContextExceeded,
    /// A request and its generation reserve do not fit, even after compaction
    /// removed every reducible turn.
    #[error(
        "request needs {used} input tokens plus {reserve} output tokens; the {limit}-token context limit is exceeded even after compaction. If the limit is a provisional working budget (the model's real context window is unknown), set the profile's context_limit to the actual serving window; otherwise reduce the task's evidence or select a model with a larger context"
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
    RunBudget {
        resource: super::BudgetResource,
        reason: String,
    },
    /// Required recording failure.
    #[error("runtime recording failed: {reason}")]
    Recording { reason: String },
    /// Host refused a tool before effects.
    #[error("tool `{tool}` is not permitted: {reason}")]
    ToolPolicy { tool: String, reason: String },
    /// Host could not render a tool before effects.
    #[error("could not render tool `{tool}`: {reason}")]
    ToolRender { tool: String, reason: String },
    /// A required approved local resource was unavailable before dispatch.
    #[error("tool `{tool}` requires a local prerequisite: {reason}")]
    ToolPrerequisite { tool: String, reason: String },
    /// The installed galla runner could not be resolved or bound before dispatch.
    #[error("the installed galla runner is unavailable: {reason}")]
    RunnerUnavailable { reason: String },
    /// The runner's verified enforcement backend was incomplete before dispatch.
    #[error("the verified enforcement backend is unavailable: {reason}")]
    BackendUnavailable { reason: String },
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
        match error {
            crate::Error::ModelTransportCancelled => Self::Cancelled,
            crate::Error::ModelContextExceeded => Self::ModelContextExceeded,
            crate::Error::RepetitiveGeneration { report } => Self::RepetitiveGeneration {
                report,
                usage: None,
            },
            error => Self::ModelTransport {
                model: None,
                reason: error.to_string(),
            },
        }
    }
}
