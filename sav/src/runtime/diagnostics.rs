//! Bounded operational evidence; classifications never authorize tool replay.

use super::{Error, ToolOutcome};
use serde::Serialize;

/// The explicit allowance which stopped work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetResource {
    WallTime,
    ModelTokens,
    Turns,
    ModelAttempts,
    Context,
}

/// The first observed reason for ending a native prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "resource", rename_all = "snake_case")]
pub enum StopReason {
    Completed,
    NoWork,
    Cancelled,
    Limit(BudgetResource),
    Stalled,
    Failed,
    /// Guidance required: recovery budget exhausted but work remains.
    GuidanceRequired,
}

/// Stable failure domains, separate from a process's untrusted output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureCategory {
    InvalidRequest,
    Denied,
    Prerequisite,
    Timeout,
    OutputLimit,
    Cancelled,
    ProcessExit,
    Unobserved,
    Stalled,
}

/// What failure evidence establishes about possible effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectCertainty {
    NotExecuted,
    MayHaveEffects,
}

/// Suggested recovery, never permission to retry or broaden authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    ReviseRequest,
    InspectState,
    FixPrerequisite,
    ReduceOutput,
    ReviewLimits,
    StopAndReconcile,
}

/// Printable failure evidence and a safe next action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolFailure {
    pub category: ToolFailureCategory,
    pub effects: EffectCertainty,
    pub message: String,
    pub recovery: RecoveryAction,
    pub next_step: String,
}

impl ToolFailure {
    /// Describes an explicit refusal before dispatch.
    pub fn not_executed(category: ToolFailureCategory, message: &str) -> Self {
        let mut failure = Self::new(category, EffectCertainty::NotExecuted, message);
        failure.next_step = format!("Not executed; {}", failure.next_step);
        failure
    }

    /// Classifies observed failure flags without guessing OOM or rollback.
    pub fn from_outcome(outcome: &ToolOutcome) -> Option<Self> {
        if !outcome.failed() {
            return None;
        }
        let (category, cause) = if outcome.cancelled {
            (
                ToolFailureCategory::Cancelled,
                "tool execution cancelled".into(),
            )
        } else if outcome.timed_out {
            (
                ToolFailureCategory::Timeout,
                "tool wall-time limit reached".into(),
            )
        } else if outcome.output_limit_exceeded {
            (
                ToolFailureCategory::OutputLimit,
                "tool output limit reached".into(),
            )
        } else if !outcome.exited || outcome.status.is_none() {
            (
                ToolFailureCategory::Unobserved,
                "tool did not report an exit status".into(),
            )
        } else {
            (
                ToolFailureCategory::ProcessExit,
                format!("tool exited unsuccessfully (status {:?})", outcome.status),
            )
        };
        let detail = if outcome.stderr.is_empty() {
            outcome.output_text(4096)
        } else {
            outcome.error_text(4096)
        };
        Some(Self::new(
            category,
            EffectCertainty::MayHaveEffects,
            &format!("{cause}: {detail}"),
        ))
    }

    /// Host errors are uncertain unless their domain explicitly rejects dispatch.
    pub fn from_error(error: &Error) -> Self {
        match error {
            Error::Cancelled => Self::new(
                ToolFailureCategory::Cancelled,
                EffectCertainty::MayHaveEffects,
                &error.describe(),
            ),
            Error::ToolPolicy { .. } => {
                Self::not_executed(ToolFailureCategory::Denied, &error.describe())
            }
            Error::ToolRender { .. } => {
                Self::not_executed(ToolFailureCategory::InvalidRequest, &error.describe())
            }
            Error::ToolPrerequisite { .. } => {
                Self::not_executed(ToolFailureCategory::Prerequisite, &error.describe())
            }
            _ => Self::new(
                ToolFailureCategory::Unobserved,
                EffectCertainty::MayHaveEffects,
                &error.describe(),
            ),
        }
    }

    /// Bounded readable form suitable for a host's redacted operational notice.
    pub fn describe(&self) -> String {
        format!(
            "{:?} / {:?}: {}\nNext: {}",
            self.category, self.effects, self.message, self.next_step
        )
    }

    fn new(category: ToolFailureCategory, effects: EffectCertainty, message: &str) -> Self {
        let (recovery, next_step) = match category {
            ToolFailureCategory::InvalidRequest => (
                RecoveryAction::ReviseRequest,
                "correct the arguments using the tool contract; do not repeat the unchanged request.",
            ),
            ToolFailureCategory::Denied => (
                RecoveryAction::ReviseRequest,
                "choose an action within the existing grant; additional authority requires human approval.",
            ),
            ToolFailureCategory::Prerequisite => (
                RecoveryAction::FixPrerequisite,
                "restore the approved local prerequisite; do not download resources or broaden grants automatically.",
            ),
            ToolFailureCategory::Timeout => (
                RecoveryAction::ReviewLimits,
                "reconcile possible partial effects, then narrow the work or review the configured wall-time limit.",
            ),
            ToolFailureCategory::OutputLimit => (
                RecoveryAction::ReduceOutput,
                "reconcile possible partial effects and request narrower output within the existing capture limit.",
            ),
            ToolFailureCategory::Cancelled | ToolFailureCategory::Unobserved => (
                RecoveryAction::StopAndReconcile,
                "stop and reconcile possible effects before a human-approved new action; never blindly replay. Missing status does not establish OOM.",
            ),
            ToolFailureCategory::ProcessExit | ToolFailureCategory::Stalled => (
                RecoveryAction::InspectState,
                "inspect the diagnostics and current state, then choose a materially different supported action; do not blindly replay.",
            ),
        };
        Self {
            category,
            effects,
            message: printable(message),
            recovery,
            next_step: next_step.into(),
        }
    }
}

fn printable(text: &str) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        let piece = if ch.is_control() && ch != '\n' && ch != '\t' {
            ch.escape_default().to_string()
        } else {
            ch.to_string()
        };
        if result.len().saturating_add(piece.len()) > 4096 {
            break;
        }
        result.push_str(&piece);
    }
    result
}
