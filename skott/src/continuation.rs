//! Skott's automatic-continuation policy.
//!
//! The agent loop in the shared Sav runtime ends every prompt with a single
//! [`StopReason`]. Some of those reasons mean the model returned control
//! *without* finishing the task and *without* a legitimate block: it simply
//! stopped (`NoWork`), or a tool failure ended the run (`Failed`). Today a
//! human has to notice that and type "continue" — the supervision step this
//! module automates.
//!
//! Skott is the component that knows the stop reason, the turn count, and the
//! task framing, so Skott owns the policy and authors the guidance. The worker
//! applies [`continuation_decision`] after each run and, on [`Continue`],
//! resubmits a focused continuation prompt through the same session, bounded
//! by [`MAX_CONTINUATIONS`] so a genuinely stuck agent surfaces for a human
//! decision instead of looping forever.
//!
//! Only three terminal states are acceptable: the task is **finished**
//! ([`StopReason::Completed`]), the agent is **blocked** for a concrete reason
//! or awaiting a human decision (cancellation, a budget limit, a stall, or
//! exhausted recovery guidance), or — for every other stop — Skott ensures
//! continued progress by **continuing** the run.
//!
//! Kvist embeds Skott and applies a stricter completion policy for task runs
//! (it also forces a stalled task run to continue and records each
//! continuation in the attempt journal). Kvist therefore disables the worker's
//! automatic continuation and keeps its own task-aware loop on top of this
//! shared policy.

use crate::session::{RunSummary, StopReason};

/// The maximum automatic continuations Skott submits for one prompt before it
/// stops forcing progress and surfaces the result for a human decision.
pub const MAX_CONTINUATIONS: u32 = 5;

/// Skott's completion policy for one agent run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationDecision {
    /// The agent finished the task; the run may be reported as complete.
    Finished,
    /// The agent is blocked for a concrete reason or needs a human decision;
    /// the run may be reported as blocked or awaiting decision.
    Blocked,
    /// The agent returned control without finishing and without a legitimate
    /// block; Skott resubmits a continuation prompt to keep it working.
    Continue,
}

/// Maps a Skott [`StopReason`] to the completion policy.
///
/// `Completed` is the only accepted success. Cancellation, a budget limit, a
/// stall, and exhausted recovery guidance are accepted blocks: the agent is
/// stopped for a concrete reason or awaiting a human decision, so Skott
/// surfaces the result rather than forcing more work. Every other stop — the
/// model returning control without finishing (`NoWork`) or a tool failure
/// ending the run (`Failed`) — is a continuation: Skott keeps the task moving.
pub fn continuation_decision(stop_reason: StopReason) -> ContinuationDecision {
    match stop_reason {
        StopReason::Completed => ContinuationDecision::Finished,
        StopReason::Cancelled
        | StopReason::Stalled
        | StopReason::GuidanceRequired
        | StopReason::Limit(_) => ContinuationDecision::Blocked,
        StopReason::NoWork | StopReason::Failed => ContinuationDecision::Continue,
    }
}

/// Builds the constructive continuation prompt Skott hands back to the agent.
///
/// The message explains *what happened* (the stop reason and how many turns
/// ran) and *what to do* (reconcile what is done, then keep working until one
/// of the three accepted outcomes). It is deliberately informative and
/// forward-looking: it does not blame the model, it does not restate the whole
/// task, and it names the accepted end states so the agent knows exactly what
/// "done" means.
pub fn continuation_prompt(summary: &RunSummary, attempt: u32, max: u32) -> String {
    let stop = summary
        .stop_reason
        .map(|reason| format!("{reason:?}"))
        .unwrap_or_else(|| "no work".into());
    format!(
        "The task is not finished yet. It stopped with reason {stop} after {} turn(s) \
         without reaching an accepted outcome. Reconcile what you have already done, \
         then keep working until the task is complete, you are blocked for a concrete \
         reason, or a human decision is required — those are the only accepted outcomes. \
         (automatic continuation {attempt}/{max})",
        summary.turns,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_completion_is_finished() {
        assert_eq!(
            continuation_decision(StopReason::Completed),
            ContinuationDecision::Finished
        );
    }

    #[test]
    fn concrete_blocks_are_not_forced() {
        for reason in [
            StopReason::Cancelled,
            StopReason::Stalled,
            StopReason::GuidanceRequired,
            StopReason::Limit(crate::session::BudgetResource::WallTime),
            StopReason::Limit(crate::session::BudgetResource::Turns),
            StopReason::Limit(crate::session::BudgetResource::ModelTokens),
        ] {
            assert_eq!(
                continuation_decision(reason),
                ContinuationDecision::Blocked,
                "{reason:?}"
            );
        }
    }

    #[test]
    fn unfinished_stops_continue() {
        for reason in [StopReason::NoWork, StopReason::Failed] {
            assert_eq!(
                continuation_decision(reason),
                ContinuationDecision::Continue,
                "{reason:?}"
            );
        }
    }

    #[test]
    fn prompt_is_constructive_and_bounded() {
        let summary = RunSummary {
            stop_reason: Some(StopReason::NoWork),
            turns: 7,
            ..RunSummary::default()
        };
        let prompt = continuation_prompt(&summary, 2, MAX_CONTINUATIONS);
        assert!(prompt.contains("NoWork"), "names the stop reason: {prompt}");
        assert!(
            prompt.contains("7 turn"),
            "reports the turn count: {prompt}"
        );
        assert!(
            prompt.contains("2/5"),
            "reports the bounded attempt: {prompt}"
        );
        assert!(
            prompt.contains("complete")
                && prompt.contains("blocked")
                && prompt.contains("human decision"),
            "names the accepted outcomes: {prompt}"
        );
    }
}
