//! Bounded, printable operator diagnostics; never execution authority.

use sav::runtime::{BudgetResource, EffectCertainty, StopReason, ToolFailure};

const MAX_DIAGNOSTIC_BYTES: usize = 4096;

pub(crate) fn printable(text: &str) -> String {
    let mut end = text.len().min(MAX_DIAGNOSTIC_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let escaped = crate::error::terminal_text(&text[..end]);
    let mut visible_end = escaped.len().min(MAX_DIAGNOSTIC_BYTES);
    while !escaped.is_char_boundary(visible_end) {
        visible_end -= 1;
    }
    let mut result = escaped[..visible_end].to_owned();
    if end < text.len() || visible_end < escaped.len() {
        result.push_str(" [truncated]");
    }
    result
}

pub(crate) fn resource_label(resource: BudgetResource) -> &'static str {
    match resource {
        BudgetResource::WallTime => "wall_time",
        BudgetResource::ModelTokens => "model_tokens",
        BudgetResource::Turns => "turns",
        BudgetResource::ModelAttempts => "model_attempts",
        BudgetResource::Context => "context",
    }
}

pub(crate) fn safe_failure(failure: &ToolFailure) -> ToolFailure {
    ToolFailure {
        category: failure.category,
        effects: failure.effects,
        message: printable(&failure.message),
        recovery: failure.recovery,
        next_step: printable(&failure.next_step),
    }
}

pub(crate) fn failure_text(description: &str, failure: &ToolFailure) -> String {
    let failure = safe_failure(failure);
    let category = serde_json::to_value(failure.category)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unobserved".into());
    let effects = match failure.effects {
        EffectCertainty::NotExecuted => "not_executed",
        EffectCertainty::MayHaveEffects => "may_have_effects",
    };
    format!(
        "✗ tool {} failed [{category}; {effects}]: {}\nNext: {}",
        printable(description),
        failure.message,
        failure.next_step
    )
}

pub(crate) fn stop_text(reason: StopReason) -> (&'static str, String) {
    match reason {
        StopReason::Completed => ("done", "completed".into()),
        StopReason::NoWork => ("awaiting", "no answer this turn — send a follow-up to continue".into()),
        StopReason::Cancelled => ("cancelled", "cancelled — completed effects are not rolled back; inspect state before continuing".into()),
        StopReason::Limit(resource) => ("exhausted", format!(
            "prompt limit reached [{}] — review this resource limit and inspect completed work before continuing",
            resource_label(resource))),
        StopReason::Stalled => ("stalled", "stalled — inspect completed work and revise the request; do not blindly replay uncertain effects".into()),
        StopReason::Failed => ("error", "failed — inspect diagnostics and reconcile possible effects before continuing".into()),
    }
}
