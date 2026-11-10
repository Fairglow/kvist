use sav::{
    CancellationToken,
    loop_detection::{RecoveryBudget, StreamLoopDetector, ToolLoopTracker},
    runtime::{StopReason, BudgetResource},
};

use super::{TokenBudget, TurnBudget, cancellation::SignalCancellation};
use crate::{Result, config::AgentProfile};

/// Unique task owner: verification may consume time, never renew model resources.
pub(crate) struct AuthoringSequence {
    pub(super) wall: TurnBudget,
    pub(super) tokens: TokenBudget,
    pub(super) recovery: RecoveryBudget,
    pub(super) tools: ToolLoopTracker,
    pub(super) generation: StreamLoopDetector,
    pub(super) cancellation: CancellationToken,
    pub(crate) last_stop: StopReason,
    pub(crate) current_log_path: Option<std::path::PathBuf>,
    pub(super) corrections_used: u8,
    pub(crate) repairs_started: usize,
    pub(super) awaiting_decision: bool,
    _signals: SignalCancellation,
}

#[derive(serde::Serialize)]
pub(crate) struct SequenceDiagnostics {
    pub elapsed_seconds: f64,
    pub remaining_seconds: f64,
    pub model_charge: u64,
    pub remaining_model_tokens: Option<u64>,
    accounting: super::accounting::Accounting,
    pub stop_reason: Option<StopReason>,
    pub disposition: &'static str,
    pub stall_corrections_used: u8,
    pub remaining_stall_corrections: u8,
    pub remaining_repair_rounds: usize,
    pub next_step: &'static str,
}

impl AuthoringSequence {
    pub(crate) fn new(profile: &AgentProfile) -> Result<Self> {
        Self::with_cancellation(profile, CancellationToken::new())
    }

    pub(crate) fn with_cancellation(
        profile: &AgentProfile,
        cancellation: CancellationToken,
    ) -> Result<Self> {
        let wall = TurnBudget::new(std::time::Duration::from_secs(profile.timeout_seconds));
        let signals = SignalCancellation::start(&cancellation)?;
        Ok(Self {
            wall,
            tokens: TokenBudget::new(profile.token_limit),
            recovery: RecoveryBudget::default(),
            tools: ToolLoopTracker::default(),
            generation: StreamLoopDetector::default(),
            cancellation,
            last_stop: StopReason::NoWork,
            current_log_path: None,
            corrections_used: 0,
            repairs_started: 0,
            awaiting_decision: false,
            _signals: signals,
        })
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.awaiting_decision {
            return Err(crate::KvistError::Runtime(sav::runtime::Error::InvalidModelTurn {
                reason: "retained decision requires human resolution; this sequence cannot schedule more work".into(),
            }));
        }
        self.wall.model_deadline(&self.cancellation)?;
        self.tokens.check()
    }

    pub(crate) fn known_usage(&self) -> Option<sav::ModelUsage> {
        self.tokens.known_usage()
    }

    pub(crate) fn diagnostics(&self, stop_reason: StopReason) -> SequenceDiagnostics {
        SequenceDiagnostics {
            elapsed_seconds: self.wall.started.elapsed().as_secs_f64(),
            remaining_seconds: self.wall.remaining().as_secs_f64(),
            model_charge: self.tokens.charge(),
            remaining_model_tokens: self.tokens.remaining(),
            accounting: self.tokens.accounting(),
            stop_reason: if self.awaiting_decision {
                None
            } else {
                Some(stop_reason)
            },
            disposition: if self.awaiting_decision {
                "awaiting_decision"
            } else if stop_reason == StopReason::Completed {
                "authoring_completed"
            } else {
                "stopped"
            },
            stall_corrections_used: self.corrections_used,
            remaining_stall_corrections: 2u8.saturating_sub(self.corrections_used),
            remaining_repair_rounds: 4usize.saturating_sub(self.repairs_started),
            next_step: if self.awaiting_decision {
                "Resolve the retained decision through the human decision workflow; do not continue authoring or verification automatically."
            } else if stop_reason == StopReason::Completed {
                "Inspect retained changes and verification evidence; execution success is not human acceptance."
            } else {
                "Inspect retained verification, effects and decisions; reconcile uncertain state before an explicitly approved new attempt. Review configured limits only with human approval."
            },
        }
    }
}

impl SequenceDiagnostics {
    pub(crate) fn message(&self) -> String {
        format!(
            "Authoring sequence: elapsed {:.2}s; remaining {:.2}s; model charge {} tokens; remaining model allowance {:?}; stall corrections {}/2 used ({} remaining); repair rounds {} remaining; stop {:?}. Next step: {}",
            self.elapsed_seconds,
            self.remaining_seconds,
            self.model_charge,
            self.remaining_model_tokens,
            self.stall_corrections_used,
            self.remaining_stall_corrections,
            self.remaining_repair_rounds,
            self.stop_reason,
            self.next_step,
        )
    }
}

pub(crate) fn error_stop(error: &crate::KvistError) -> StopReason {
    use sav::runtime::BudgetResource;
    match error {
        crate::KvistError::Runtime(sav::runtime::Error::Cancelled) => StopReason::Cancelled,
        crate::KvistError::Runtime(sav::runtime::Error::RunBudget { resource, .. }) => {
            StopReason::Limit(*resource)
        }
        crate::KvistError::Sav(source)
        | crate::KvistError::AgentModelRequestFailed { source, .. } => match source {
            sav::Error::Cancelled | sav::Error::ModelTransportCancelled => StopReason::Cancelled,
            sav::Error::ModelTransportTimedOut
            | sav::Error::SlotAllocationTimedOut { .. }
            | sav::Error::TtftTimedOut { .. }
            | sav::Error::InterTokenCadenceTimedOut { .. } => {
                StopReason::Limit(BudgetResource::WallTime)
            }
            sav::Error::RepetitiveGeneration { .. } => StopReason::Stalled,
            _ => StopReason::Failed,
        },
        _ => StopReason::Failed,
    }
}

/// Helper to determine if a stop reason represents a real blocker that should
/// prevent continuation. Returns `true` if the work is genuinely blocked and
/// cannot proceed without human intervention.
pub(crate) fn is_real_blocker(stop_reason: StopReason) -> bool {
    match stop_reason {
        // These are genuine blockers - the agent cannot continue
        StopReason::Cancelled => true,
        StopReason::Limit(BudgetResource::WallTime) => true,
        StopReason::Limit(BudgetResource::ModelTokens) => true,
        StopReason::Limit(BudgetResource::Turns) => true,
        StopReason::Limit(BudgetResource::ModelAttempts) => true,
        StopReason::Limit(BudgetResource::Context) => true,
        StopReason::Stalled => true, // Agent is stuck, needs human help
        StopReason::GuidanceRequired => true, // Guidance needed but work remains
        
        // These are NOT blockers - the agent should continue or complete
        StopReason::Completed => false,
        StopReason::NoWork => false, // Nothing to do, task is done
        StopReason::Failed => false, // Failure doesn't mean blocked; can retry or reconcile
    }
}