use sav::runtime::{EffectCertainty, RecoveryAction, ToolFailure, ToolFailureCategory};

use super::{EffectRecord, LiveRun, redact_bounded};
use crate::Result;

pub(super) fn record(
    live: &LiveRun,
    name: &str,
    call_id: &str,
    category: ToolFailureCategory,
    effects: EffectCertainty,
    message: &str,
    redactions: &[String],
) -> Result<()> {
    let uncertain = effects == EffectCertainty::MayHaveEffects;
    let failure = ToolFailure {
        category,
        effects,
        message: redact_bounded(
            &redact_bounded(message, redactions, 4096)
                .chars().flat_map(char::escape_debug).collect::<String>(),
            &[], 4096,
        ),
        recovery: if uncertain {
            RecoveryAction::StopAndReconcile
        } else {
            RecoveryAction::ReviseRequest
        },
        next_step: if uncertain {
            "Inspect retained effects and current revisions; reconcile before any explicitly approved new attempt. Do not replay automatically."
        } else {
            "Revise the request within the existing approved grant; no additional authority is implied."
        }.into(),
    };
    live.record(
        &serde_json::json!({"type":"tool_failure","data":{
            "name":name, "call_id":call_id, "failure":failure
        }}),
        true,
    )?;
    Ok(())
}

pub(super) fn effect(
    live: &LiveRun,
    record: &EffectRecord,
    cancelled: bool,
    redactions: &[String],
) -> Result<()> {
    let category = if cancelled {
        ToolFailureCategory::Cancelled
    } else if record.timed_out {
        ToolFailureCategory::Timeout
    } else if record.output_limit_exceeded {
        ToolFailureCategory::OutputLimit
    } else {
        ToolFailureCategory::ProcessExit
    };
    self::record(
        live,
        &record.tool,
        &record.call_id,
        category,
        EffectCertainty::MayHaveEffects,
        &record.detail,
        redactions,
    )
}
