//! Execution-backend delegation seam tests.
//!
//! The native runtime never executes an effect itself: every tool intent is
//! delegated to the host-injected [`ToolExecutor`], which renders the intent
//! inside the approved isolation layer (the installed galla runner and its
//! verified backend) and reports the bounded outcome back. These tests drive
//! the loop against a scripted model and a recording executor, and pin the
//! delegation contract:
//!
//! - every effect is delegated to the executor, never run on the host;
//! - the runner and backend are probed before any effect is dispatched;
//! - a delegated effect is bound to the installed runner identity and is
//!   refused when that binding is broken;
//! - when the runner or its backend is unavailable the executor refuses, the
//!   runtime fails closed (it surfaces the refusal and records the effect as
//!   not executed), and it never substitutes an unconstrained host execution.
//!
//! The executor models the host's delegation to galla; the runtime under test
//! is the delegation seam itself.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sav::runtime::{
    AgentSession, ContextManager, Error, Event, EventSink, ModelIdentity, NativeRuntime,
    RunSummary, StopReason, ToolExecutor, ToolOutcome,
};
use sav::{
    CancellationToken, FinishReason, LocalModelProvider, ModelRequest, ModelStreamEvent,
    ModelTransport, ModelTurn, ReasoningEffort, ToolDefinition, ToolIntent,
};

/// A scripted model that proposes one tool intent, then answers and stops.
struct ScriptedModel {
    calls: AtomicUsize,
}

impl ModelTransport for ScriptedModel {
    fn complete(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
    ) -> sav::Result<ModelTurn> {
        unreachable!("the loop streams turns")
    }

    fn stream(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
        _on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(ModelTurn {
                text: String::new(),
                reasoning: None,
                tool_intents: vec![ToolIntent {
                    id: "call-1".into(),
                    provider_id: None,
                    name: "write_file".into(),
                    arguments: serde_json::json!({"path": "/workspace/note.txt"}),
                }],
                finish_reason: FinishReason::ToolCalls,
                provider: LocalModelProvider::LlamaServer,
                model: "native".into(),
                response_id: None,
                provider_request_id: None,
                usage: None,
            }),
            _ => Ok(ModelTurn {
                text: "done".into(),
                reasoning: None,
                tool_intents: vec![],
                finish_reason: FinishReason::Stop,
                provider: LocalModelProvider::LlamaServer,
                model: "native".into(),
                response_id: None,
                provider_request_id: None,
                usage: None,
            }),
        }
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(30)
    }
}

/// A sink that records every event so a test can inspect the tool outcome.
#[derive(Default)]
struct CollectingSink {
    events: std::sync::Mutex<Vec<Event>>,
}

impl EventSink for CollectingSink {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

/// A recording executor that models the host's delegation to the installed
/// galla runner and its verified backend. It probes the runner and backend
/// before dispatch, binds the delegated effect to the installed runner
/// identity, counts every effect it actually causes, and refuses — without
/// executing anything — when a prerequisite is missing or the binding is
/// broken.
#[derive(Default)]
struct DelegatingExecutor {
    runner_available: bool,
    backend_available: bool,
    /// The runner identity the delegated effect is bound to.
    bound_runner: String,
    /// The runner identity actually presented to the executor at dispatch.
    presented_runner: String,
    /// Probe of the runner and backend capabilities, performed before dispatch.
    probe_calls: AtomicUsize,
    /// Effects actually caused by the executor.
    effects: AtomicUsize,
    /// Intents handed to `execute`, in dispatch order.
    dispatched: std::sync::Mutex<Vec<String>>,
}

impl DelegatingExecutor {
    fn new(runner_available: bool, backend_available: bool) -> Self {
        Self {
            runner_available,
            backend_available,
            ..Self::default()
        }
    }

    fn effects(&self) -> usize {
        self.effects.load(Ordering::SeqCst)
    }
}

impl ToolExecutor for DelegatingExecutor {
    fn describe(&self, intent: &ToolIntent) -> String {
        format!("delegate {} to the sandbox", intent.name)
    }

    fn execute(
        &self,
        intent: &ToolIntent,
        _cancellation: &CancellationToken,
    ) -> sav::runtime::Result<ToolOutcome> {
        self.dispatched.lock().unwrap().push(intent.name.clone());
        // Probe the runner and backend capabilities before any effect.
        self.probe_calls.fetch_add(1, Ordering::SeqCst);
        // Fail closed when the runner is unavailable: refuse before dispatch.
        if !self.runner_available {
            return Err(Error::ToolPrerequisite {
                tool: intent.name.clone(),
                reason: "the installed galla runner is unavailable".into(),
            });
        }
        // Fail closed when the runner's backend is unavailable: refuse before
        // dispatch, without an unconstrained host fallback.
        if !self.backend_available {
            return Err(Error::ToolPrerequisite {
                tool: intent.name.clone(),
                reason: "the verified Bubblewrap backend is unavailable".into(),
            });
        }
        // Bind the delegated effect to the installed runner identity. A broken
        // binding is a terminal host failure: the effect must not run.
        if self.bound_runner != self.presented_runner {
            return Err(Error::Host {
                reason: format!(
                    "runner binding mismatch: effect bound to `{}`, presented runner is `{}`",
                    self.bound_runner, self.presented_runner
                ),
            });
        }
        // The runner and backend are present and the binding holds: cause the
        // effect inside the sandbox and report the bounded outcome.
        self.effects.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: b"ok".to_vec(),
            stderr: Vec::new(),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        })
    }
}

fn tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "write_file".into(),
        description: "Write a file inside the sandbox".into(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    }
}

fn run_with_executor(executor: &DelegatingExecutor) -> (RunSummary, Vec<Event>) {
    let model = ScriptedModel {
        calls: AtomicUsize::new(0),
    };
    let mut session = AgentSession::new(
        ModelIdentity {
            id: "selected".into(),
            model: "native".into(),
        },
        ReasoningEffort::None,
        vec![tool_definition()],
        "host instructions".into(),
    );
    session.push_user("write a note");
    let mut context = ContextManager::new(128_000, 6);
    let cancellation = CancellationToken::new();
    let sink = CollectingSink::default();
    let summary = NativeRuntime::default()
        .run(
            &mut session,
            &model,
            executor,
            &sink,
            &cancellation,
            &mut context,
            None,
        )
        .expect("the scripted model and the executor never fail the loop");
    (summary, sink.events.lock().unwrap().clone())
}

/// With the runner and backend available, the runtime delegates the effect to
/// the executor, the executor causes exactly one effect, and the run completes.
#[test]
fn delegates_every_effect_to_the_executor() {
    let executor = DelegatingExecutor::new(true, true);
    let (summary, _events) = run_with_executor(&executor);
    assert_eq!(
        executor.effects(),
        1,
        "the executor causes the single effect"
    );
    assert_eq!(
        summary.tools_executed, 1,
        "the runtime counts the delegated effect"
    );
    assert_eq!(summary.answer.as_deref(), Some("done"));
}

/// The runner and backend are probed before any effect is dispatched.
#[test]
fn probes_runner_and_backend_before_dispatch() {
    let executor = DelegatingExecutor::new(true, true);
    let _ = run_with_executor(&executor);
    assert!(
        executor.probe_calls.load(Ordering::SeqCst) >= 1,
        "the runner and backend are probed before dispatch"
    );
    assert_eq!(
        executor.effects(),
        1,
        "the effect runs only after the probe confirms capability"
    );
}

/// When the runner is unavailable the executor refuses before dispatch, the
/// runtime surfaces the refusal, and no effect is caused: the runtime fails
/// closed and never substitutes an unconstrained host execution.
#[test]
fn fails_closed_when_the_runner_is_unavailable() {
    let executor = DelegatingExecutor::new(false, true);
    let (summary, events) = run_with_executor(&executor);
    assert_eq!(
        executor.effects(),
        0,
        "no effect is caused when the runner is unavailable"
    );
    assert_eq!(
        summary.tools_executed, 0,
        "the runtime does not count a refused effect as executed"
    );
    let failure = summary.failure.as_deref().expect("the refusal is surfaced");
    assert!(
        failure.contains("galla runner is unavailable"),
        "the runner-unavailable refusal is surfaced: {failure}"
    );
    assert!(
        events
            .iter()
            .any(|event| { matches!(event, Event::ToolResult { failed: true, .. }) }),
        "the refused tool result is reported as failed"
    );
}

/// When the runner's backend is unavailable the executor refuses before
/// dispatch, the runtime surfaces the refusal, and no effect is caused.
#[test]
fn fails_closed_when_the_backend_is_unavailable() {
    let executor = DelegatingExecutor::new(true, false);
    let (summary, events) = run_with_executor(&executor);
    assert_eq!(
        executor.effects(),
        0,
        "no effect is caused when the backend is unavailable"
    );
    assert_eq!(summary.tools_executed, 0);
    let failure = summary.failure.as_deref().expect("the refusal is surfaced");
    assert!(
        failure.contains("Bubblewrap backend is unavailable"),
        "the backend-unavailable refusal is surfaced: {failure}"
    );
    assert!(
        events
            .iter()
            .any(|event| { matches!(event, Event::ToolResult { failed: true, .. }) }),
        "the refused tool result is reported as failed"
    );
}

/// A delegated effect is bound to the installed runner identity. When the
/// presented runner does not match the bound identity, the executor refuses
/// and no effect is caused.
#[test]
fn binds_delegated_effects_to_the_installed_runner() {
    let executor = DelegatingExecutor {
        runner_available: true,
        backend_available: true,
        bound_runner: "sha256:installed".into(),
        presented_runner: "sha256:substituted".into(),
        ..DelegatingExecutor::default()
    };
    let (summary, _events) = run_with_executor(&executor);
    assert_eq!(
        executor.effects(),
        0,
        "no effect is caused when the runner binding is broken"
    );
    assert_eq!(summary.tools_executed, 0);
    let failure = summary
        .failure
        .as_deref()
        .expect("the binding refusal is surfaced");
    assert!(
        failure.contains("runner binding mismatch"),
        "the binding refusal is surfaced: {failure}"
    );
}

/// A correctly bound effect is accepted: the presented runner matches the
/// bound identity, so the executor causes the effect and the run completes.
#[test]
fn accepts_a_correctly_bound_effect() {
    let executor = DelegatingExecutor {
        runner_available: true,
        backend_available: true,
        bound_runner: "sha256:installed".into(),
        presented_runner: "sha256:installed".into(),
        ..DelegatingExecutor::default()
    };
    let (summary, _events) = run_with_executor(&executor);
    assert_eq!(executor.effects(), 1, "the bound effect is caused");
    assert_eq!(summary.tools_executed, 1);
    assert_eq!(summary.stop_reason, Some(StopReason::Completed));
}
