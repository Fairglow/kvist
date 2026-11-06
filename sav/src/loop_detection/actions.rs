use super::{RecoveryStep, compute_action_hash};
use crate::{ToolIntent, runtime::ToolOutcome};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

const HISTORY_LIMIT: usize = 128;
const CYCLE_LIMIT: usize = 32;

/// Evidence supplied by the host, never inferred from a model's progress claim.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressEvidence {
    /// No independently observed revision change.
    #[default]
    None,
    /// The host verified that relevant state changed.
    StateChanged,
}

/// The bounded evidence causing a tool intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolLoopKind {
    /// An effect proposal recurred without observed state change.
    RepeatedAction,
    /// A repeatable action returned the same complete observation.
    RepeatedObservation,
    /// Attempts repeatedly returned an identical classified failure.
    RepeatedFailure,
    /// A sequence of actions and outcomes recurred.
    ActionCycle,
}

/// Metadata-only tool recurrence report; contains no captured output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolLoopReport {
    /// The detected evidence category.
    pub kind: ToolLoopKind,
    /// The number of actions in the repeating period.
    pub period: usize,
    /// The number of observed/proposed copies.
    pub copies: usize,
}
impl ToolLoopReport {
    /// Goal-preserving feedback for one actual correction opportunity.
    pub fn feedback(&self, step: RecoveryStep) -> String {
        format!(
            "Recovery {}/{}: repeated tool activity ({:?}, period {}, copies {}). \
             Earlier completed effects and the original goal are retained. \
             Reconcile current state, inspect a genuinely new observation, or choose a \
             different supported approach. Do not blindly replay uncertain effects. \
             Successful no-op output is not progress; authority and resource limits are unchanged.",
            step.attempt, step.limit, self.kind, self.period, self.copies,
        )
    }
    /// Honest terminal guidance after the prompt's correction allowance is spent.
    pub fn exhausted_message(&self) -> String {
        format!(
            "Repeated-action circuit breaker: tool-loop stalled after two autonomous corrections \
             ({:?}, period {}). Earlier completed effects remain; reconcile current state \
             and supply focused guidance or change the supported approach before continuing.",
            self.kind, self.period,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    action: String,
    outcome: [u8; 32],
    failed: bool,
}

/// Bounded progress-aware recurrence evidence. Recovery ownership remains with the host.
#[derive(Debug, Clone, Default)]
pub struct ToolLoopTracker {
    history: VecDeque<Record>,
}
impl ToolLoopTracker {
    /// Whether a successful complete observation is new to this bounded history.
    pub fn is_new_observation(&self, intent: &ToolIntent, outcome: &ToolOutcome) -> bool {
        if outcome.failed() {
            return false;
        }
        let action = compute_action_hash(&intent.name, &intent.arguments);
        let fingerprint = outcome_fingerprint(outcome);
        !self
            .history
            .iter()
            .any(|record| record.action == action && record.outcome == fingerprint)
    }
    /// Checks a nonrepeatable proposal before effects. Reads require actual outcome comparison.
    pub fn propose(&self, intent: &ToolIntent, repeatable: bool) -> Option<ToolLoopReport> {
        if repeatable {
            return None;
        }
        let action = compute_action_hash(&intent.name, &intent.arguments);
        let count = self
            .history
            .iter()
            .filter(|record| record.action == action)
            .count();
        if self
            .history
            .back()
            .is_some_and(|record| record.action == action)
            || count >= 2
        {
            Some(ToolLoopReport {
                kind: ToolLoopKind::RepeatedAction,
                period: 1,
                copies: count + 1,
            })
        } else {
            None
        }
    }

    /// Records executed/refused evidence once; only a validated revision clears history.
    pub fn observe(
        &mut self,
        intent: &ToolIntent,
        outcome: &ToolOutcome,
        repeatable: bool,
        progress: ProgressEvidence,
    ) -> Option<ToolLoopReport> {
        if progress == ProgressEvidence::StateChanged {
            self.history.clear();
        }
        let record = Record {
            action: compute_action_hash(&intent.name, &intent.arguments),
            outcome: outcome_fingerprint(outcome),
            failed: outcome.failed(),
        };
        let observed_copies = self
            .history
            .iter()
            .filter(|prior| prior.action == record.action && prior.outcome == record.outcome)
            .count()
            + 1;
        let failure_copies = if record.failed {
            self.history
                .iter()
                .filter(|prior| prior.failed && prior.outcome == record.outcome)
                .count()
                + 1
        } else {
            0
        };
        if self.history.len() == HISTORY_LIMIT {
            self.history.pop_front();
        }
        self.history.push_back(record);
        if failure_copies >= 3 {
            return Some(ToolLoopReport {
                kind: ToolLoopKind::RepeatedFailure,
                period: 1,
                copies: failure_copies,
            });
        }
        if repeatable && observed_copies >= 3 {
            return Some(ToolLoopReport {
                kind: ToolLoopKind::RepeatedObservation,
                period: 1,
                copies: observed_copies,
            });
        }
        let tail: Vec<_> = self.history.iter().rev().take(CYCLE_LIMIT * 2).collect();
        for period in 2..=CYCLE_LIMIT.min(tail.len() / 2) {
            if tail[..period] == tail[period..period * 2] {
                return Some(ToolLoopReport {
                    kind: ToolLoopKind::ActionCycle,
                    period,
                    copies: 2,
                });
            }
        }
        None
    }

    /// Number of retained action/outcome fingerprints, never greater than 128.
    pub fn history_len(&self) -> usize {
        self.history.len()
    }
}

fn outcome_fingerprint(outcome: &ToolOutcome) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update([
        u8::from(outcome.exited),
        u8::from(outcome.status.is_some()),
        u8::from(outcome.timed_out),
        u8::from(outcome.output_limit_exceeded),
        u8::from(outcome.cancelled),
    ]);
    if let Some(status) = outcome.status {
        hash.update(status.to_le_bytes());
    }
    for bytes in [&outcome.stdout, &outcome.stderr] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.finalize().into()
}
