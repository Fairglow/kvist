use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use sav::runtime::{
    AgentSession, ContextManager, ControlMailbox, Event, EventSink, GuidanceState, ModelIdentity,
    NativeRuntime, Recorder, RetryPolicy, RunLimits, RunSummary, ToolExecutor, ToolOutcome,
};
use sav::{
    CancellationToken, FinishReason, LocalModelProvider, ModelMessage, ModelRequest,
    ModelStreamEvent, ModelTransport, ModelTurn, ReasoningEffort, ToolIntent,
};

fn answer() -> ModelTurn {
    ModelTurn {
        text: "done".into(),
        reasoning: None,
        tool_intents: vec![],
        finish_reason: FinishReason::Stop,
        provider: LocalModelProvider::LlamaServer,
        model: "test".into(),
        response_id: None,
        provider_request_id: None,
        usage: None,
    }
}
fn session() -> AgentSession {
    let mut session = AgentSession::new(
        ModelIdentity {
            id: "test".into(),
            model: "test".into(),
        },
        ReasoningEffort::None,
        vec![],
        "host authority".into(),
    );
    session.push_user("original objective");
    session
}
#[derive(Default)]
struct Sink(Mutex<Vec<Event>>);
impl EventSink for Sink {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}
struct NeverExecute;
impl ToolExecutor for NeverExecute {
    fn execute(&self, _: &ToolIntent, _: &CancellationToken) -> sav::runtime::Result<ToolOutcome> {
        panic!("provisional tool must never execute")
    }
}
struct QuietProvider<'a> {
    controls: &'a ControlMailbox,
    calls: AtomicUsize,
}
impl ModelTransport for QuietProvider<'_> {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!()
    }
    fn stream(
        &self,
        request: &ModelRequest,
        token: &CancellationToken,
        emit: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            emit(ModelStreamEvent::TextDelta("provisional".into()))?;
            self.controls.steer("additional guidance").unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            while !token.is_cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(
                token.is_cancelled(),
                "quiet provider must observe operation interruption"
            );
            return Err(sav::Error::ModelTransportCancelled);
        }
        assert!(
            request
                .messages
                .contains(&ModelMessage::User("original objective".into()))
        );
        assert!(
            request
                .messages
                .contains(&ModelMessage::User("additional guidance".into()))
        );
        assert!(
            !request
                .messages
                .iter()
                .any(|m| matches!(m, ModelMessage::Assistant{text,..} if text == "provisional"))
        );
        Ok(answer())
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}
#[test]
fn quiet_generation_is_interrupted_without_cancelling_prompt() {
    let controls = ControlMailbox::default();
    let provider = QuietProvider {
        controls: &controls,
        calls: AtomicUsize::new(0),
    };
    let token = CancellationToken::new();
    let sink = Sink::default();
    let result = NativeRuntime::with_retry(
        5,
        RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(1)),
    )
    .run_controlled(
        &mut session(),
        &provider,
        &NeverExecute,
        &sink,
        &token,
        &mut ContextManager::new(8192, 4),
        None,
        &controls,
    )
    .unwrap();
    assert!(result.success(), "{result:?}");
    assert!(!token.is_cancelled());
    let states: Vec<_> = sink
        .0
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            Event::Control(notice) => Some((notice.id, notice.state)),
            _ => None,
        })
        .collect();
    assert_eq!(states.len(), 3);
    assert!(states.iter().all(|(id, _)| *id == states[0].0));
    assert_eq!(
        states.iter().map(|(_, state)| *state).collect::<Vec<_>>(),
        vec![
            GuidanceState::Accepted,
            GuidanceState::Claimed,
            GuidanceState::Delivered
        ]
    );
}

#[test]
fn mailbox_bounds_edits_and_withdrawals_are_atomic_before_claim() {
    let controls = ControlMailbox::default();
    controls.begin_run().unwrap();
    let first = controls.steer("first").unwrap();
    controls.edit(first, "edited").unwrap();
    controls.withdraw(first).unwrap();
    assert!(controls.edit(first, "too late").is_err());
    for _ in 0..32 {
        controls.steer("bounded").unwrap();
    }
    assert!(controls.steer("overflow").is_err());
    controls.shutdown();
    assert!(controls.steer("after shutdown").is_err());
}

fn success() -> ToolOutcome {
    ToolOutcome {
        exited: true,
        status: Some(0),
        stdout: b"retained effect".to_vec(),
        stderr: vec![],
        timed_out: false,
        output_limit_exceeded: false,
        cancelled: false,
    }
}
fn intent(id: &str) -> ToolIntent {
    ToolIntent {
        id: id.into(),
        provider_id: None,
        name: "effect".into(),
        arguments: serde_json::json!({"id":id}),
    }
}
struct ToolProvider {
    calls: AtomicUsize,
    requests: Mutex<Vec<ModelRequest>>,
}

#[test]
fn duplicate_read_notices_forget_old_observations_without_blocking_reads() {
    struct Reads(AtomicUsize);
    impl ModelTransport for Reads {
        fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
            let call = self.0.fetch_add(1, Ordering::SeqCst);
            let page = match call {
                0 | 1 | 1026 => 0,
                2..=1025 => call - 1,
                1027 => 1024,
                _ => return Ok(answer()),
            };
            let mut turn = answer();
            turn.tool_intents = vec![ToolIntent {
                id: format!("read-{call}"),
                provider_id: None,
                name: "read".into(),
                arguments: serde_json::json!({"page":page}),
            }];
            turn.finish_reason = FinishReason::ToolCalls;
            Ok(turn)
        }
        fn stream(
            &self,
            request: &ModelRequest,
            cancellation: &CancellationToken,
            _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            self.complete(request, cancellation)
        }
        fn deadline(&self) -> Duration {
            Duration::from_secs(5)
        }
    }
    struct Reader;
    impl ToolExecutor for Reader {
        fn execute(
            &self,
            _: &ToolIntent,
            _: &CancellationToken,
        ) -> sav::runtime::Result<ToolOutcome> {
            Ok(success())
        }
        fn repeatable(&self, _: &ToolIntent) -> bool {
            true
        }
        fn repeated_observation_notice(&self, _: &ToolIntent) -> Option<String> {
            Some("recent duplicate read".into())
        }
    }
    let sink = Sink::default();
    let result = NativeRuntime::default()
        .run(
            &mut session(),
            &Reads(AtomicUsize::new(0)),
            &Reader,
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(8192, 4),
            None,
        )
        .unwrap();
    assert!(result.success(), "{result:?}");
    assert_eq!(
        sink.0
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, Event::Note(note) if note == "recent duplicate read"))
            .count(),
        2
    );
}
impl ModelTransport for ToolProvider {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!()
    }
    fn stream(
        &self,
        request: &ModelRequest,
        _: &CancellationToken,
        _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        self.requests.lock().unwrap().push(request.clone());
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            let mut turn = answer();
            turn.text.clear();
            turn.finish_reason = FinishReason::ToolCalls;
            turn.tool_intents = vec![intent("one"), intent("two")];
            Ok(turn)
        } else {
            Ok(answer())
        }
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}
struct SteerDuringEffect<'a> {
    controls: &'a ControlMailbox,
    calls: AtomicUsize,
    shutdown: bool,
}
impl ToolExecutor for SteerDuringEffect<'_> {
    fn execute(
        &self,
        _: &ToolIntent,
        token: &CancellationToken,
    ) -> sav::runtime::Result<ToolOutcome> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let id = self.controls.steer("first guidance").unwrap();
        self.controls.edit(id, "edited first guidance").unwrap();
        let withdrawn = self.controls.steer("withdraw me").unwrap();
        self.controls.withdraw(withdrawn).unwrap();
        self.controls.steer("second guidance").unwrap();
        assert!(
            !token.is_cancelled(),
            "steering must not interrupt an executing effect"
        );
        if self.shutdown {
            self.controls.shutdown();
        }
        Ok(success())
    }
}
#[test]
fn effect_finishes_stale_calls_are_paired_then_ordered_guidance_reaches_provider() {
    let controls = ControlMailbox::default();
    let provider = ToolProvider {
        calls: AtomicUsize::new(0),
        requests: Mutex::new(vec![]),
    };
    let executor = SteerDuringEffect {
        controls: &controls,
        calls: AtomicUsize::new(0),
        shutdown: false,
    };
    let mut conversation = session();
    let summary = NativeRuntime::default()
        .run_controlled(
            &mut conversation,
            &provider,
            &executor,
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(8192, 4),
            None,
            &controls,
        )
        .unwrap();
    assert!(summary.success());
    assert_eq!(summary.tools_executed, 1);
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
    let requests = provider.requests.lock().unwrap();
    let messages = &requests[1].messages;
    let one = messages
        .iter()
        .position(|m| {
            matches!(m,ModelMessage::ToolResult{call_id, content,..}
        if call_id=="one" && content.contains("retained effect"))
        })
        .unwrap();
    let two = messages
        .iter()
        .position(|m| {
            matches!(m,ModelMessage::ToolResult{call_id, content,..}
        if call_id=="two" && content.contains("not executed"))
        })
        .unwrap();
    let first = messages
        .iter()
        .position(|m| *m == ModelMessage::User("edited first guidance".into()))
        .unwrap();
    let second = messages
        .iter()
        .position(|m| *m == ModelMessage::User("second guidance".into()))
        .unwrap();
    assert!(one < two && two < first && first < second);
    assert!(!messages.contains(&ModelMessage::User("withdraw me".into())));
    assert!(controls.edit(sav::runtime::GuidanceId(1), "late").is_err());
}
#[test]
fn shutdown_during_effect_retains_result_but_never_delivers_or_restarts() {
    let controls = ControlMailbox::default();
    let provider = ToolProvider {
        calls: AtomicUsize::new(0),
        requests: Mutex::new(vec![]),
    };
    let executor = SteerDuringEffect {
        controls: &controls,
        calls: AtomicUsize::new(0),
        shutdown: true,
    };
    let mut conversation = session();
    let summary = NativeRuntime::default()
        .run_controlled(
            &mut conversation,
            &provider,
            &executor,
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(8192, 4),
            None,
            &controls,
        )
        .unwrap();
    assert!(summary.cancelled);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
    assert!(
        !conversation
            .messages()
            .contains(&ModelMessage::User("second guidance".into()))
    );
}
struct RetryProvider(AtomicUsize);
impl ModelTransport for RetryProvider {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!()
    }
    fn stream(
        &self,
        request: &ModelRequest,
        _: &CancellationToken,
        _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(sav::Error::ModelTransportTimedOut);
        }
        assert!(
            request
                .messages
                .contains(&ModelMessage::User("backoff guidance".into()))
        );
        Ok(answer())
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}
struct BackoffSink<'a> {
    controls: &'a ControlMailbox,
    events: Mutex<Vec<Event>>,
}
impl EventSink for BackoffSink<'_> {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        if matches!(&event,Event::Note(text) if text.contains("retrying")) {
            self.controls.steer("backoff guidance")?;
        }
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}
#[test]
fn guidance_interrupts_backoff_and_renews_generation_attempts() {
    let controls = ControlMailbox::default();
    let sink = BackoffSink {
        controls: &controls,
        events: Mutex::new(vec![]),
    };
    let provider = RetryProvider(AtomicUsize::new(0));
    let start = Instant::now();
    let summary = NativeRuntime::with_retry(
        5,
        RetryPolicy::new(2, Duration::from_secs(5), Duration::from_secs(5)),
    )
    .run_controlled(
        &mut session(),
        &provider,
        &NeverExecute,
        &sink,
        &CancellationToken::new(),
        &mut ContextManager::new(8192, 4),
        None,
        &controls,
    )
    .unwrap();
    assert!(summary.success());
    assert!(start.elapsed() < Duration::from_secs(1));
    let attempts: Vec<_> = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            Event::AttemptStart { attempt } => Some(*attempt),
            _ => None,
        })
        .collect();
    assert_eq!(attempts, vec![1, 1]);
}
struct FailingDelivery {
    delivered: bool,
    fail: bool,
    fail_event: bool,
    dispatch_guidance: Option<ControlMailbox>,
}
impl Recorder for FailingDelivery {
    fn model_event(&mut self, _: &ModelStreamEvent) -> sav::runtime::Result<()> {
        if self.fail_event {
            return Err(sav::runtime::Error::Recording {
                reason: "live event persistence failure".into(),
            });
        }
        Ok(())
    }
    fn control(&mut self, notice: &sav::runtime::GuidanceNotice) -> sav::runtime::Result<()> {
        if notice.state == GuidanceState::Delivered {
            self.delivered = true;
            if self.fail {
                return Err(sav::runtime::Error::Recording {
                    reason: "delivery persistence failure".into(),
                });
            }
        }
        Ok(())
    }
    fn session_start(&mut self) -> sav::runtime::Result<()> {
        Ok(())
    }
    fn request(&mut self, _: &ModelRequest, _: u32) -> sav::runtime::Result<()> {
        Ok(())
    }
    fn turn_start(&mut self, _: &ModelTurn) -> sav::runtime::Result<usize> {
        Ok(0)
    }
    fn turn_finish(
        &mut self,
        _: usize,
        _: &Option<sav::ModelUsage>,
        _: &str,
    ) -> sav::runtime::Result<()> {
        Ok(())
    }
    fn tool_dispatch(&mut self, _: usize, _: &ToolIntent) -> sav::runtime::Result<()> {
        if let Some(controls) = self.dispatch_guidance.take() {
            controls.steer("guidance while synchronizing dispatch")?;
        }
        Ok(())
    }
    fn tool_result(
        &mut self,
        _: usize,
        _: &str,
        _: &str,
        _: &serde_json::Value,
        _: &ToolOutcome,
    ) -> sav::runtime::Result<()> {
        Ok(())
    }
    fn session_finish(&mut self, _: &RunSummary) -> sav::runtime::Result<()> {
        Ok(())
    }
}
#[test]
fn delivery_recording_failure_stops_before_another_provider_or_effect() {
    let controls = ControlMailbox::default();
    let provider = QuietProvider {
        controls: &controls,
        calls: AtomicUsize::new(0),
    };
    let mut recorder = FailingDelivery {
        delivered: false,
        fail: true,
        fail_event: false,
        dispatch_guidance: None,
    };
    let result = NativeRuntime::default().run_controlled(
        &mut session(),
        &provider,
        &NeverExecute,
        &Sink::default(),
        &CancellationToken::new(),
        &mut ContextManager::new(8192, 4),
        Some(&mut recorder),
        &controls,
    );
    assert!(matches!(result, Err(sav::runtime::Error::Recording { .. })));
    assert!(recorder.delivered);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(!controls.is_active());
}
#[test]
fn guidance_accepted_during_dispatch_recording_skips_every_stale_effect() {
    let controls = ControlMailbox::default();
    let provider = ToolProvider {
        calls: AtomicUsize::new(0),
        requests: Mutex::new(vec![]),
    };
    let mut recorder = FailingDelivery {
        delivered: false,
        fail: false,
        fail_event: false,
        dispatch_guidance: Some(controls.clone()),
    };
    let summary = NativeRuntime::default()
        .run_controlled(
            &mut session(),
            &provider,
            &NeverExecute,
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(8192, 4),
            Some(&mut recorder),
            &controls,
        )
        .unwrap();
    assert!(summary.success());
    assert_eq!(summary.tools_executed, 0);
    assert!(recorder.delivered);
    let requests = provider.requests.lock().unwrap();
    assert_eq!(
        requests[1]
            .messages
            .iter()
            .filter(|m| matches!(m,
        ModelMessage::ToolResult{content,..} if content.contains("not executed")))
            .count(),
        2
    );
}

#[test]
fn live_model_event_recording_failure_stops_before_presentation_or_effects() {
    let controls = ControlMailbox::default();
    let provider = QuietProvider {
        controls: &controls,
        calls: AtomicUsize::new(0),
    };
    let mut recorder = FailingDelivery {
        delivered: false,
        fail: false,
        fail_event: true,
        dispatch_guidance: None,
    };
    let sink = Sink::default();
    let result = NativeRuntime::default().run_controlled(
        &mut session(),
        &provider,
        &NeverExecute,
        &sink,
        &CancellationToken::new(),
        &mut ContextManager::new(8192, 4),
        Some(&mut recorder),
        &controls,
    );
    assert!(matches!(result, Err(sav::runtime::Error::Recording { .. })));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(
        !sink
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, Event::Text(_)))
    );
}

struct WithdrawDuringGeneration<'a>(&'a ControlMailbox, AtomicUsize);
impl ModelTransport for WithdrawDuringGeneration<'_> {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!()
    }
    fn stream(
        &self,
        request: &ModelRequest,
        _: &CancellationToken,
        emit: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        if self.1.fetch_add(1, Ordering::SeqCst) == 0 {
            let id = self.0.steer("withdrawn guidance").unwrap();
            assert!(emit(ModelStreamEvent::TextDelta("discard this partial".into())).is_err());
            self.0.withdraw(id).unwrap();
            return Err(sav::Error::ModelTransportCancelled);
        }
        assert!(
            !request
                .messages
                .contains(&ModelMessage::User("withdrawn guidance".into()))
        );
        Ok(answer())
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}
#[test]
fn withdrawal_after_operation_interrupt_does_not_cancel_the_prompt() {
    let controls = ControlMailbox::default();
    let provider = WithdrawDuringGeneration(&controls, AtomicUsize::new(0));
    let token = CancellationToken::new();
    let summary = NativeRuntime::default()
        .run_controlled(
            &mut session(),
            &provider,
            &NeverExecute,
            &Sink::default(),
            &token,
            &mut ContextManager::new(8192, 4),
            None,
            &controls,
        )
        .unwrap();
    assert!(summary.success());
    assert!(!token.is_cancelled());
    assert_eq!(provider.1.load(Ordering::SeqCst), 2);
}

struct FinalRaceProvider<'a> {
    controls: &'a ControlMailbox,
    calls: AtomicUsize,
}
impl ModelTransport for FinalRaceProvider<'_> {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!()
    }
    fn stream(
        &self,
        request: &ModelRequest,
        _: &CancellationToken,
        _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            let receipt = self
                .controls
                .submit("guidance racing final answer")
                .unwrap();
            assert_eq!(receipt.kind, sav::runtime::SubmissionKind::Guidance);
        } else {
            assert!(
                request
                    .messages
                    .contains(&ModelMessage::User("guidance racing final answer".into()))
            );
        }
        Ok(answer())
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}
#[test]
fn guidance_racing_final_answer_cannot_become_a_replacement_task() {
    let controls = ControlMailbox::default();
    let provider = FinalRaceProvider {
        controls: &controls,
        calls: AtomicUsize::new(0),
    };
    let summary = NativeRuntime::default()
        .run_controlled(
            &mut session(),
            &provider,
            &NeverExecute,
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(8192, 4),
            None,
            &controls,
        )
        .unwrap();
    assert!(summary.success());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert_eq!(controls.next_task().unwrap(), None);
}

#[test]
fn repeated_human_guidance_renews_turn_and_generation_allowances() {
    struct Guided<'a> {
        controls: &'a ControlMailbox,
        calls: AtomicUsize,
    }
    impl ModelTransport for Guided<'_> {
        fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
            unreachable!()
        }
        fn stream(
            &self,
            request: &ModelRequest,
            _: &CancellationToken,
            _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call < 8 {
                self.controls
                    .steer(format!("human guidance {call}"))
                    .unwrap();
                return Err(sav::Error::ModelTransportCancelled);
            }
            assert!(
                request
                    .messages
                    .contains(&ModelMessage::User("human guidance 7".into()))
            );
            Ok(answer())
        }
        fn deadline(&self) -> Duration {
            Duration::from_secs(5)
        }
    }
    let controls = ControlMailbox::default();
    let provider = Guided {
        controls: &controls,
        calls: AtomicUsize::new(0),
    };
    let summary = NativeRuntime::with_retry(1, RetryPolicy::default())
        .run_controlled(
            &mut session(),
            &provider,
            &NeverExecute,
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::for_model(8192),
            None,
            &controls,
        )
        .unwrap();
    assert!(summary.success(), "{summary:?}");
    assert_eq!(summary.turns, 9);
}

#[test]
fn new_tasks_reset_explicit_token_budgets_and_invalid_limits_fail() {
    let limits = RunLimits::default();
    assert_eq!(limits.wall_time, None);
    assert_eq!(limits.max_tokens, None);
    for limits in [
        RunLimits {
            wall_time: Some(Duration::ZERO),
            ..limits
        },
        RunLimits {
            wall_time: Some(Duration::MAX),
            ..limits
        },
        RunLimits {
            max_tokens: Some(0),
            ..limits
        },
    ] {
        assert!(limits.validate().is_err());
    }
    RunLimits {
        wall_time: Some(Duration::from_secs(7 * 86400)),
        ..limits
    }
    .validate()
    .unwrap();
    let mut conversation = session();
    let mut context = ContextManager::for_model(8192);
    for _ in 0..2 {
        conversation.push_user("new task");
        let controls = ControlMailbox::default();
        let provider = FinalRaceProvider {
            controls: &controls,
            calls: AtomicUsize::new(1),
        };
        conversation.push_user("guidance racing final answer");
        let mut request = conversation.next_request().unwrap();
        request.max_output_tokens = Some(1024);
        let charge = sav::runtime::context::estimate_request(&request) as u64 + 1024;
        let summary = NativeRuntime::default()
            .with_limits(RunLimits {
                max_tokens: Some(charge),
                ..limits
            })
            .unwrap()
            .run_controlled(
                &mut conversation,
                &provider,
                &NeverExecute,
                &Sink::default(),
                &CancellationToken::new(),
                &mut context,
                None,
                &controls,
            )
            .unwrap();
        assert!(summary.success(), "{summary:?}");
        assert_eq!(summary.turns, 1);
    }
}
#[test]
fn interrupted_generation_keeps_token_charge_and_prompt_deadline() {
    let controls = ControlMailbox::default();
    let provider = QuietProvider {
        controls: &controls,
        calls: AtomicUsize::new(0),
    };
    let mut request = session().next_request().unwrap();
    request.max_output_tokens = Some(1024);
    let charge = sav::runtime::context::estimate_request(&request) as u64 + 1024;
    let summary = NativeRuntime::default()
        .with_limits(RunLimits {
            max_tokens: Some(charge),
            ..RunLimits::default()
        })
        .unwrap()
        .run_controlled(
            &mut session(),
            &provider,
            &NeverExecute,
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(8192, 4),
            None,
            &controls,
        )
        .unwrap();
    assert!(summary.budget_exhausted);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
#[test]
fn completion_and_submission_classification_have_one_linearization_point() {
    let controls = ControlMailbox::default();
    let task = controls.submit("objective").unwrap();
    assert_eq!(task.kind, sav::runtime::SubmissionKind::Task);
    assert_eq!(controls.next_task().unwrap().as_deref(), Some("objective"));
    let steering = controls.submit("guidance before completion").unwrap();
    assert_eq!(steering.kind, sav::runtime::SubmissionKind::Guidance);
    assert!(!controls.finish_if_idle().unwrap());
    let claimed = controls.claim().unwrap();
    assert_eq!(claimed[0].id, steering.id);
    assert!(controls.withdraw(steering.id).is_err());
    assert!(controls.finish_if_idle().unwrap());
    let next = controls.submit("objective after completion").unwrap();
    assert_eq!(next.kind, sav::runtime::SubmissionKind::Task);
    assert!(!controls.has_pending());
}

struct EndlessSteering<'a>(&'a ControlMailbox, AtomicUsize);
impl ModelTransport for EndlessSteering<'_> {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!()
    }
    fn stream(
        &self,
        _: &ModelRequest,
        _: &CancellationToken,
        _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        self.1.fetch_add(1, Ordering::SeqCst);
        self.0.steer("more guidance").unwrap();
        Err(sav::Error::ModelTransportCancelled)
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}
#[test]
fn repeated_guidance_cannot_reset_explicit_token_limit() {
    let controls = ControlMailbox::default();
    let provider = EndlessSteering(&controls, AtomicUsize::new(0));
    let result = NativeRuntime::with_retry(
        50,
        RetryPolicy::new(2, Duration::from_millis(1), Duration::from_millis(1)),
    )
    .with_limits(RunLimits {
        max_tokens: Some(8000),
        ..RunLimits::default()
    })
    .unwrap()
    .run_controlled(
        &mut session(),
        &provider,
        &NeverExecute,
        &Sink::default(),
        &CancellationToken::new(),
        &mut ContextManager::new(8192, 4),
        None,
        &controls,
    );
    assert!(result.unwrap().budget_exhausted);
    assert!(provider.1.load(Ordering::SeqCst) > 2);
    assert!(provider.1.load(Ordering::SeqCst) < 8);
    assert_eq!(controls.next_task().unwrap(), None);
}
struct DeadlineProvider<'a>(&'a ControlMailbox, AtomicUsize);
impl ModelTransport for DeadlineProvider<'_> {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!()
    }
    fn stream(
        &self,
        _: &ModelRequest,
        token: &CancellationToken,
        _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        if self.1.fetch_add(1, Ordering::SeqCst) == 0 {
            self.0.steer("guidance does not reset deadline").unwrap();
        }
        while !token.is_cancelled() {
            std::thread::sleep(Duration::from_millis(1));
        }
        Err(sav::Error::ModelTransportCancelled)
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}
#[test]
fn steering_preserves_wall_deadline() {
    let controls = ControlMailbox::default();
    let provider = DeadlineProvider(&controls, AtomicUsize::new(0));
    let start = Instant::now();
    let summary = NativeRuntime::default()
        .with_limits(RunLimits {
            wall_time: Some(Duration::from_millis(80)),
            ..RunLimits::default()
        })
        .unwrap()
        .run_controlled(
            &mut session(),
            &provider,
            &NeverExecute,
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(8192, 4),
            None,
            &controls,
        )
        .unwrap();
    assert!(summary.budget_exhausted);
    assert_eq!(provider.1.load(Ordering::SeqCst), 2);
    assert!(start.elapsed() < Duration::from_secs(1));
}
