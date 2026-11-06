use sav::runtime::{
    AgentSession, ContextManager, Event, EventSink, ModelIdentity, NativeRuntime, RetryPolicy,
    ToolExecutor, ToolOutcome,
};
use sav::{
    CancellationToken, FinishReason, LocalModelProvider, ModelRequest, ModelTransport, ModelTurn,
    ReasoningEffort, ToolDefinition, ToolIntent,
};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

struct GuidanceDuringToolRecovery {
    controls: sav::runtime::ControlMailbox,
    sent: std::sync::atomic::AtomicBool,
}
impl EventSink for GuidanceDuringToolRecovery {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        if matches!(event, Event::Note(ref text) if text.contains("Recovery "))
            && !self.sent.swap(true, Ordering::SeqCst)
        {
            self.controls
                .steer("Focused human help alongside correction.")?;
        }
        Ok(())
    }
}
#[test]
fn guidance_during_tool_feedback_preserves_a_real_combined_correction() {
    let provider = Provider {
        batches: (0..10)
            .map(|batch| (0..5).map(|n| intent(batch * 5 + n, 0)).collect())
            .collect(),
        requests: Mutex::new(Vec::new()),
        repeated_text: None,
    };
    let executor = Executor {
        calls: AtomicUsize::new(0),
        repeatable: false,
        distinct: false,
        failed: false,
        hard_failure_at: None,
    };
    let controls = sav::runtime::ControlMailbox::default();
    let sink = GuidanceDuringToolRecovery {
        controls: controls.clone(),
        sent: std::sync::atomic::AtomicBool::new(false),
    };
    let result =
        NativeRuntime::with_retry(None, RetryPolicy::new(1, Duration::ZERO, Duration::ZERO))
            .run_controlled(
                &mut fixture_session(),
                &provider,
                &executor,
                &sink,
                &CancellationToken::new(),
                &mut ContextManager::new(32_768, 4),
                None,
                &controls,
            )
            .unwrap();
    assert!(!result.success());
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    for phrase in ["Recovery 1/2", "Focused human help"] {
        assert!(requests[1].messages.iter().any(|message| matches!(message,
            sav::ModelMessage::User(text) if text.contains(phrase)
        )));
    }
    assert!(requests[2].messages.iter().any(|message| matches!(message,
        sav::ModelMessage::User(text) if text.contains("Recovery 2/2")
    )));
}

struct Provider {
    batches: Vec<Vec<ToolIntent>>,
    requests: Mutex<Vec<ModelRequest>>,
    repeated_text: Option<String>,
}
impl ModelTransport for Provider {
    fn complete(&self, request: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        requests.push(request.clone());
        let tools = self.batches.get(index).cloned().unwrap_or_default();
        Ok(ModelTurn {
            provider: LocalModelProvider::Ollama,
            model: "test".into(),
            text: self
                .repeated_text
                .clone()
                .unwrap_or_else(|| format!("Investigating step {index}.")),
            reasoning: None,
            finish_reason: if tools.is_empty() {
                FinishReason::Stop
            } else {
                FinishReason::ToolCalls
            },
            tool_intents: tools,
            usage: None,
            response_id: None,
            provider_request_id: None,
        })
    }
    fn stream(
        &self,
        request: &ModelRequest,
        token: &CancellationToken,
        _: &mut dyn FnMut(sav::ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        self.complete(request, token)
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}

struct Executor {
    calls: AtomicUsize,
    repeatable: bool,
    distinct: bool,
    failed: bool,
    hard_failure_at: Option<usize>,
}
impl ToolExecutor for Executor {
    fn repeatable(&self, _: &ToolIntent) -> bool {
        self.repeatable
    }
    fn execute(
        &self,
        intent: &ToolIntent,
        _: &CancellationToken,
    ) -> sav::runtime::Result<ToolOutcome> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(at) = self.hard_failure_at {
            let error = sav::runtime::Error::Host {
                reason: "terminal executor failure".into(),
            };
            if call >= at {
                return Err(error);
            }
            return Ok(ToolOutcome::rejected_with(error.describe()));
        }
        Ok(ToolOutcome {
            exited: true,
            status: Some(if self.failed { 1 } else { 0 }),
            stdout: if self.distinct {
                intent.arguments.to_string().into_bytes()
            } else {
                b"unchanged".to_vec()
            },
            stderr: Vec::new(),
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        })
    }
}
#[derive(Default)]
struct Sink(Mutex<Vec<Event>>);
impl EventSink for Sink {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}
fn intent(id: usize, path: usize) -> ToolIntent {
    ToolIntent {
        id: format!("call-{id}"),
        provider_id: None,
        name: "probe".into(),
        arguments: serde_json::json!({"path": path}),
    }
}
fn run(
    batches: Vec<Vec<ToolIntent>>,
    repeatable: bool,
    distinct: bool,
    failed: bool,
) -> (sav::runtime::RunSummary, Provider, Executor, Sink) {
    let provider = Provider {
        batches,
        requests: Mutex::new(Vec::new()),
        repeated_text: None,
    };
    run_provider(provider, repeatable, distinct, failed)
}
fn run_provider(
    provider: Provider,
    repeatable: bool,
    distinct: bool,
    failed: bool,
) -> (sav::runtime::RunSummary, Provider, Executor, Sink) {
    let executor = Executor {
        calls: AtomicUsize::new(0),
        repeatable,
        distinct,
        failed,
        hard_failure_at: None,
    };
    run_executor(provider, executor)
}
fn run_executor(
    provider: Provider,
    executor: Executor,
) -> (sav::runtime::RunSummary, Provider, Executor, Sink) {
    let sink = Sink::default();
    let mut session = fixture_session();
    let runtime =
        NativeRuntime::with_retry(None, RetryPolicy::new(1, Duration::ZERO, Duration::ZERO));
    let result = runtime
        .run(
            &mut session,
            &provider,
            &executor,
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    (result, provider, executor, sink)
}
fn fixture_session() -> AgentSession {
    let mut session = AgentSession::new(
        ModelIdentity {
            id: "test".into(),
            model: "test".into(),
        },
        ReasoningEffort::None,
        vec![ToolDefinition {
            name: "probe".into(),
            description: "Probe fixture".into(),
            parameters: serde_json::json!({"type":"object"}),
        }],
        String::new(),
    );
    session.push_user("Keep the original objective and investigate.");
    session
}

#[test]
fn unchanged_repeatable_observations_receive_finite_correction() {
    let (result, provider, executor, _) = run(
        (0..20).map(|n| vec![intent(n, 0)]).collect(),
        true,
        false,
        false,
    );
    assert!(!result.success(), "{result:?}");
    assert!(provider.requests.lock().unwrap().len() <= 6);
    assert!(executor.calls.load(Ordering::SeqCst) <= 5);
}

#[test]
fn recurring_failed_discovery_is_not_exempt_from_stagnation() {
    let (result, provider, _, _) = run(
        (0..20).map(|n| vec![intent(n, 0)]).collect(),
        true,
        false,
        true,
    );
    assert!(!result.success(), "{result:?}");
    assert!(provider.requests.lock().unwrap().len() <= 6);
}

#[test]
fn nine_action_observation_cycles_do_not_evade_the_window() {
    let (result, provider, _, _) = run(
        (0..40).map(|n| vec![intent(n, n % 9)]).collect(),
        true,
        true,
        false,
    );
    assert!(!result.success(), "{result:?}");
    assert!(provider.requests.lock().unwrap().len() <= 24);
}

#[test]
fn one_duplicate_batch_cannot_spend_all_correction_opportunities() {
    let batches = (0..10)
        .map(|batch| (0..5).map(|n| intent(batch * 5 + n, 0)).collect())
        .collect();
    let (result, provider, executor, sink) = run(batches, false, false, false);
    assert!(!result.success(), "{result:?}");
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 3, "two real responses before exhaustion");
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
    for step in [1, 2] {
        assert!(requests.iter().any(|request| request.messages.iter().any(|message| {
            matches!(message, sav::ModelMessage::User(text) if text.contains(&format!("Recovery {step}/2")))
        })));
    }
    let events = sink.0.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::ToolResult { .. }))
            .count(),
        15
    );
}

#[test]
fn successful_noop_output_cannot_clear_effect_recurrence() {
    let batches = (0..20)
        .map(|n| vec![intent(n, if n % 2 == 0 { 0 } else { n })])
        .collect();
    let (result, provider, _, _) = run(batches, false, false, false);
    assert!(!result.success(), "{result:?}");
    assert!(provider.requests.lock().unwrap().len() < 20);
}

#[test]
fn distinct_observation_pages_remain_productive_without_a_turn_cap() {
    let (result, provider, executor, _) = run(
        (0..160).map(|n| vec![intent(n, n)]).collect(),
        true,
        true,
        false,
    );
    assert!(result.success(), "{result:?}");
    assert_eq!(provider.requests.lock().unwrap().len(), 161);
    assert_eq!(executor.calls.load(Ordering::SeqCst), 160);
}

#[test]
fn repeated_passages_across_unique_noop_turns_are_not_progress() {
    let provider = Provider {
        batches: (0..10).map(|n| vec![intent(n, n)]).collect(),
        requests: Mutex::new(Vec::new()),
        repeated_text: Some((0..150).map(|n| format!("investigationword{n} ")).collect()),
    };
    let (result, provider, executor, _) = run_provider(provider, false, false, false);
    assert!(!result.success(), "{result:?}");
    assert!(provider.requests.lock().unwrap().len() <= 5);
    assert!(executor.calls.load(Ordering::SeqCst) <= 2);
}

#[test]
fn repeated_preambles_with_new_observations_remain_productive() {
    let provider = Provider {
        batches: (0..160).map(|n| vec![intent(n, n)]).collect(),
        requests: Mutex::new(Vec::new()),
        repeated_text: Some((0..150).map(|n| format!("investigationword{n} ")).collect()),
    };
    let (result, provider, executor, _) = run_provider(provider, true, true, false);
    assert!(result.success(), "{result:?}");
    assert_eq!(provider.requests.lock().unwrap().len(), 161);
    assert_eq!(executor.calls.load(Ordering::SeqCst), 160);
}

fn assert_terminal_failure(fail_at: usize) {
    let provider = Provider {
        batches: (0..10).map(|n| vec![intent(n, n)]).collect(),
        requests: Mutex::new(Vec::new()),
        repeated_text: None,
    };
    let executor = Executor {
        calls: AtomicUsize::new(0),
        repeatable: true,
        distinct: false,
        failed: true,
        hard_failure_at: Some(fail_at),
    };
    let (result, provider, executor, _) = run_executor(provider, executor);
    let original = sav::runtime::Error::Host {
        reason: "terminal executor failure".into(),
    }
    .describe();
    assert_eq!(result.failure.as_deref(), Some(original.as_str()));
    assert_eq!(provider.requests.lock().unwrap().len(), fail_at);
    assert_eq!(executor.calls.load(Ordering::SeqCst), fail_at);
}

#[test]
fn terminal_executor_error_cannot_trigger_another_recovery_request() {
    assert_terminal_failure(3);
}

#[test]
fn exhausted_tool_recovery_cannot_overwrite_terminal_executor_cause() {
    assert_terminal_failure(5);
}
