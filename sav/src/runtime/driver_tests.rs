use super::*;
use crate::runtime::ModelIdentity;
use crate::{FinishReason, LocalModelProvider};
use crate::{ReasoningEffort, ToolChoice};

#[test]
fn assistant_helper_only_accepts_normal_nonblank_final_answers() {
    let model = ModelIdentity {
        id: "test".into(),

        model: "test".into(),
    };
    for (reason, text, expected) in [
        (FinishReason::Length, "partial", None),
        (FinishReason::ContentFilter, "filtered", None),
        (FinishReason::ToolCalls, "inconsistent", None),
        (FinishReason::Other("unknown".into()), "unknown", None),
        (FinishReason::Stop, "  ", None),
        (FinishReason::Stop, "done", Some("done")),
    ] {
        let mut session =
            AgentSession::new(model.clone(), ReasoningEffort::None, vec![], String::new());
        session.apply_assistant(ModelTurn {
            text: text.into(),
            reasoning: None,
            tool_intents: vec![],
            finish_reason: reason,
            provider: LocalModelProvider::LlamaServer,
            model: "test".into(),
            response_id: None,
            provider_request_id: None,
            usage: None,
        });
        assert_eq!(session.answer.as_deref(), expected);
    }
}

/// A transport whose first `fail_first` streaming attempts fail with a
/// retryable timeout, then succeed with a terminal answer.
struct FlakyTransport {
    fail_first: u32,
    attempts: std::sync::atomic::AtomicUsize,
}

impl ModelTransport for FlakyTransport {
    fn complete(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
    ) -> crate::Result<ModelTurn> {
        Err(crate::Error::ModelTransportTimedOut)
    }

    fn stream(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
        _on_event: &mut dyn FnMut(ModelStreamEvent) -> crate::Result<()>,
    ) -> crate::Result<ModelTurn> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst) as u32;
        if attempt < self.fail_first {
            return Err(crate::Error::ModelTransportTimedOut);
        }
        Ok(ModelTurn {
            text: "done".into(),
            reasoning: None,
            tool_intents: vec![],
            finish_reason: FinishReason::Stop,
            provider: LocalModelProvider::LlamaServer,
            model: "test".into(),
            response_id: None,
            provider_request_id: None,
            usage: None,
        })
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(30)
    }
}

/// A sink that records every event so a test can assert the retry notices.
struct CollectingSink(std::sync::Mutex<Vec<Event>>);

impl EventSink for CollectingSink {
    fn send(&self, event: Event) -> Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

fn retry_test_parts<'a>(
    skott: &NativeRuntime,
    _session: &'a AgentSession,
    context: &'a ContextManager,
) -> (
    ModelRequest,
    CancellationToken,
    PromptBudget,
    LiveProgress<'a>,
) {
    let request = ModelRequest {
        model: "test".into(),
        messages: vec![],
        tools: vec![],
        tool_choice: ToolChoice::Auto,
        reasoning_effort: None,
        output_schema: None,
        max_output_tokens: None,
    };
    let cancellation = CancellationToken::new();
    let budget = PromptBudget::start(Duration::from_secs(60), &cancellation).unwrap();
    let progress = LiveProgress {
        session: _session,
        context,
        tool_defs: 0,
        started_at: Instant::now(),
        cumulative_input: 0,
        cumulative_total: 0,
        cumulative_output: 0,
        decode_seconds: 0.0,
    };
    let _ = skott;
    (request, cancellation, budget, progress)
}

#[test]
fn drive_turn_retries_transient_timeouts_then_succeeds() {
    let skott = NativeRuntime::with_retry(
        5,
        RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(1)),
    );
    let session = AgentSession::new(
        ModelIdentity {
            id: "test".into(),

            model: "test".into(),
        },
        ReasoningEffort::None,
        vec![],
        String::new(),
    );
    let context = ContextManager::new(super::super::DEFAULT_CONTEXT_TOKENS, 6);
    let (request, cancellation, budget, progress) = retry_test_parts(&skott, &session, &context);
    let transport = FlakyTransport {
        fail_first: 2,
        attempts: std::sync::atomic::AtomicUsize::new(0),
    };
    let sink = CollectingSink(std::sync::Mutex::new(Vec::new()));
    let mut tokens = 100_000_000_u64;
    let mut recorder: Option<&mut dyn Recorder> = None;

    let (turn, _) = skott
        .drive_turn(
            &request,
            &transport,
            &sink,
            &cancellation,
            &progress,
            &mut 0.0,
            &budget,
            &mut tokens,
            &mut recorder,
            &ControlMailbox::default(),
            1,
            &|intent| intent.name.clone(),
        )
        .unwrap();

    assert_eq!(turn.text, "done");
    // Two failed attempts, then the successful third.
    assert_eq!(transport.attempts.load(Ordering::SeqCst), 3);
    let events = sink.0.lock().unwrap().clone();
    // Attempt 1 starts, a retry notice is emitted, attempt 2 starts, a
    // second notice, attempt 3 starts and completes.
    let notes = events
        .iter()
        .filter(|event| matches!(event, Event::Note(_)))
        .count();
    let attempts = events
        .iter()
        .filter(|event| matches!(event, Event::AttemptStart { .. }))
        .count();
    assert_eq!(attempts, 3);
    assert_eq!(notes, 2);
    assert!(matches!(
        events.first(),
        Some(Event::AttemptStart { attempt: 1 })
    ));
}

#[test]
fn drive_turn_exhausts_the_attempt_budget_on_persistent_failures() {
    let skott = NativeRuntime::with_retry(
        5,
        RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(1)),
    );
    let session = AgentSession::new(
        ModelIdentity {
            id: "test".into(),

            model: "test".into(),
        },
        ReasoningEffort::None,
        vec![],
        String::new(),
    );
    let context = ContextManager::new(super::super::DEFAULT_CONTEXT_TOKENS, 6);
    let (request, cancellation, budget, progress) = retry_test_parts(&skott, &session, &context);
    let transport = FlakyTransport {
        fail_first: u32::MAX,
        attempts: std::sync::atomic::AtomicUsize::new(0),
    };
    let sink = CollectingSink(std::sync::Mutex::new(Vec::new()));
    let mut tokens = 100_000_000_u64;
    let mut recorder: Option<&mut dyn Recorder> = None;

    let error = skott
        .drive_turn(
            &request,
            &transport,
            &sink,
            &cancellation,
            &progress,
            &mut 0.0,
            &budget,
            &mut tokens,
            &mut recorder,
            &ControlMailbox::default(),
            1,
            &|intent| intent.name.clone(),
        )
        .err()
        .unwrap();

    // The final retryable failure surfaces once the budget is spent.
    assert!(matches!(error, Error::ModelTransport { .. }));
    // Exactly the policy's attempt budget was used, no more.
    assert_eq!(transport.attempts.load(Ordering::SeqCst), 3);
}
