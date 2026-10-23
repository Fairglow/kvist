//! Generic bounded execution outcome data.
/// The outcome of one sandbox execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutcome {
    /// Whether the process produced an exit status.
    pub exited: bool,
    /// The exit code, when the process exited.
    pub status: Option<i32>,
    /// Captured stdout bytes.
    pub stdout: Vec<u8>,
    /// Captured stderr bytes.
    pub stderr: Vec<u8>,
    /// True when the wall-clock deadline elapsed.
    pub timed_out: bool,
    /// True when the output limit was exceeded.
    pub output_limit_exceeded: bool,
    /// True when the caller requested cancellation.
    pub cancelled: bool,
}

impl ToolOutcome {
    /// A zeroed, failed outcome used to record a tool that was rejected by
    /// policy or failed before producing real output (so the record stays
    /// faithful without inventing sandbox results).
    pub fn rejected() -> Self {
        Self::rejected_with("")
    }

    /// A rejected outcome whose reason is carried in `stderr`, so a tool the
    /// loop refused (policy denial, or a failure before it produced real output)
    /// still reports *why* to the model on the next turn instead of an empty
    /// result. The process never ran, so it is not reported as exited.
    pub fn rejected_with(reason: impl Into<String>) -> Self {
        ToolOutcome {
            exited: false,
            status: None,
            stdout: Vec::new(),
            stderr: reason.into().into_bytes(),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        }
    }

    /// The captured stdout as text, truncated at `limit` bytes and lossily
    /// decoded so non-UTF-8 output never panics.
    pub fn output_text(&self, limit: usize) -> String {
        truncate_text(&self.stdout, limit)
    }

    /// The captured stderr as text, truncated at `limit` bytes.
    pub fn error_text(&self, limit: usize) -> String {
        truncate_text(&self.stderr, limit)
    }

    /// Whether execution failed to produce an observed, uninterrupted zero exit.
    pub fn failed(&self) -> bool {
        !self.exited
            || self.status != Some(0)
            || self.timed_out
            || self.output_limit_exceeded
            || self.cancelled
    }
}

fn truncate_text(bytes: &[u8], limit: usize) -> String {
    let bounded = if bytes.len() > limit {
        &bytes[..limit]
    } else {
        bytes
    };
    String::from_utf8_lossy(bounded).into_owned()
}
