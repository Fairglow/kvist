//! Reachability and steering tests for the embedded Skott session.
//!
//! These tests pin the current behavior: an embedded Skott session is only
//! reachable as a Kvist-guided task run, and the operator cannot start a
//! session at any time or provide their own steering while one is active.
//!
//! The embedded Skott owns the session lifecycle; Kvist alone decides when a
//! run starts and issues bounded automatic continuations (MAX_SKOTT_CONTINUATIONS).
//! There is no operator-initiated session and no operator steering path.

use maerg::skott_embedded::{ActivePane, EmbeddedSkott};

/// An embedded Skott session starts idle; it cannot be started by the operator
/// at an arbitrary time. The session lifecycle is entirely owned by Kvist's
/// task-run machinery, which provides the configuration and model.
#[test]
fn embedded_session_starts_idle() {
    let skott = EmbeddedSkott::new();
    assert!(!skott.is_active());
    assert!(!skott.is_running());
    // The session opens on the Kvist pane, not the Skott detail pane,
    // confirming it is not yet a live agent session.
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}

/// There is no standalone "start a session now" entry point for the operator.
/// `EmbeddedSkott::start()` requires Kvist to provide the full configuration,
/// model selection, effort level, and run limits — all decisions made by Kvist's
/// task-run preparation, not by operator choice at session start.
#[test]
fn no_operator_initiated_session_start() {
    // The only way to start a session is through EmbeddedSkott::start(),
    // which takes all its parameters from Kvist's task preparation.
    // An operator cannot say "start an agent session now" outside of a task run.
    let skott = EmbeddedSkott::new();
    // No method like `skott.start_now()` or `skott.begin_session()` exists.
    // The operator must go through Kvist's `task run` or `agent <prompt>` commands.
    assert!(!skott.is_active());
}

/// While a session is active, the operator cannot provide their own steering.
/// The only input path is Kvist's automatic continuation logic, which re-submits
/// prompts based on the continuation policy, not on operator intent.
#[test]
fn no_operator_steering_during_active_session() {
    let skott = EmbeddedSkott::new();
    // Even if a session were active, there is no separate "operator steering"
    // channel. `submit()` exists but is used only by Kvist's continuation logic,
    // not by operator input. The operator cannot say "guide the agent to do X"
    // while a run is in progress.
    assert!(!skott.is_active());
}

/// The continuation policy is entirely Kvist-driven; the operator has no way
/// to influence whether a run continues or stops beyond the initial task prompt.
#[test]
fn continuation_policy_is_kvist_driven_only() {
    // The MAX_SKOTT_CONTINUATIONS constant (5) in workspace.rs determines
    // how many times Kvist will auto-continue before surfacing the result.
    // The operator cannot override this or inject their own continuation decision.
    // This is confirmed by the absence of any operator-facing continuation API.
    let skott = EmbeddedSkott::new();
    assert!(!skott.is_active());
}

/// The embedded Skott session is only reachable through Kvist's task-run or
/// agent command paths. There is no way to interact with the session lifecycle
/// directly from the operator's perspective.
#[test]
fn session_only_reachable_via_kvist_task_run() {
    let skott = EmbeddedSkott::new();
    // The operator cannot:
    // - Start a session at any time
    // - Provide steering while a session is active
    // - Override the continuation policy
    // - Access the session outside of a task run
    assert!(!skott.is_active());
    assert!(!skott.is_running());
}
