use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use sav::runtime::{
    AgentSession, ContextManager, Event, EventSink, ModelIdentity, NativeRuntime, Recorder,
    RetryPolicy, RunLimits, RunSummary, ToolExecutor, ToolOutcome,
};
use sav::{
    CancellationToken, FinishReason, LocalModelProvider, ModelRequest, ModelStreamEvent,
    ModelTransport, ModelTurn, ReasoningEffort, ToolDefinition, ToolIntent,
};

fn turn(text: String, reasoning: Option<String>, tools: bool, length: bool) -> ModelTurn {
    ModelTurn {
        text,
        reasoning,
        tool_intents: if tools {
            vec![ToolIntent {
                id: "proposal".into(),
                provider_id: None,
                name: "effect".into(),
                arguments: serde_json::json!({"path":"owned.rs"}),
            }]
        } else {
            vec![]
        },
        finish_reason: if length {
            FinishReason::Length
        } else if tools {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        },
        provider: LocalModelProvider::LlamaServer,
        model: "fixture".into(),
        response_id: None,
        provider_request_id: None,
        usage: None,
    }
}

fn session() -> AgentSession {
    let mut session = AgentSession::new(
        ModelIdentity {
            id: "fixture".into(),
            model: "fixture".into(),
        },
        ReasoningEffort::None,
        vec![ToolDefinition {
            name: "effect".into(),
            description: "Fixture effect".into(),
            parameters: serde_json::json!({"type":"object","properties":{}}),
        }],
        "Only the host authorizes effects.".into(),
    );
    session.push_user("Complete the original goal without losing earlier work.");
    session
}

#[derive(Default)]
struct Sink(Mutex<Vec<Event>>);
fn final_progress(sink: &Sink) -> Option<Event> {
    sink.0
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|event| matches!(event, Event::Progress { .. }))
        .cloned()
}
impl EventSink for Sink {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

#[derive(Default)]
struct Executor(AtomicUsize);
impl ToolExecutor for Executor {
    fn execute(&self, _: &ToolIntent, _: &CancellationToken) -> sav::runtime::Result<ToolOutcome> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: b"fixture effect".to_vec(),
            stderr: vec![],
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        })
    }
}

struct Provider {
    text: String,
    reasoning: bool,
    unary: bool,
    tools: bool,
    length: bool,
    recover_after: Option<usize>,
    usage: Option<sav::ModelUsage>,
    calls: AtomicUsize,
    requests: Mutex<Vec<ModelRequest>>,
}
impl Provider {
    fn repeating(words: usize, reasoning: bool) -> Self {
        let passage = (0..words).map(|n| format!("word{n} ")).collect::<String>();
        Self {
            text: passage.repeat(4),
            reasoning,
            unary: false,
            tools: false,
            length: false,
            recover_after: None,
            usage: None,
            calls: AtomicUsize::new(0),
            requests: Mutex::new(vec![]),
        }
    }
}
impl ModelTransport for Provider {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!("fixture uses the streaming boundary")
    }
    fn stream(
        &self,
        request: &ModelRequest,
        operation: &CancellationToken,
        emit: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        self.requests.lock().unwrap().push(request.clone());
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.recover_after.is_some_and(|after| call >= after) {
            return Ok(turn(
                "Completed by using a different supported strategy.".into(),
                None,
                false,
                false,
            ));
        }
        if !self.unary {
            for ch in self.text.chars() {
                let event = if self.reasoning {
                    ModelStreamEvent::ReasoningDelta(ch.to_string())
                } else {
                    ModelStreamEvent::TextDelta(ch.to_string())
                };
                if let Err(error) = emit(event) {
                    assert!(
                        operation.is_cancelled(),
                        "stop the provider, not the prompt"
                    );
                    return Err(error);
                }
            }
        }
        let mut completed = turn(
            if self.reasoning {
                "Ready.".into()
            } else {
                self.text.clone()
            },
            self.reasoning.then(|| self.text.clone()),
            self.tools,
            self.length,
        );
        completed.usage = self.usage;
        Ok(completed)
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}

fn runtime() -> NativeRuntime {
    NativeRuntime::with_retry(
        10,
        RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(1)),
    )
}

#[test]
fn native_streams_stop_100_150_200_word_answer_and_reasoning_loops() {
    for words in [100, 150, 200] {
        for reasoning in [false, true] {
            let provider = Provider::repeating(words, reasoning);
            let executor = Executor::default();
            let token = CancellationToken::new();
            let sink = Sink::default();
            let result = runtime()
                .run(
                    &mut session(),
                    &provider,
                    &executor,
                    &sink,
                    &token,
                    &mut ContextManager::new(32_768, 4),
                    None,
                )
                .unwrap();
            assert!(!result.success(), "{words}/{reasoning}: {result:?}");
            assert!(result.failure.as_deref().unwrap().contains("repet"));
            assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
            assert_eq!(executor.0.load(Ordering::SeqCst), 0);
            assert!(!token.is_cancelled());
            let requests = provider.requests.lock().unwrap();
            assert!(requests.iter().all(|request| {
                request.messages.iter().any(|message| {
                    matches!(message, sav::ModelMessage::User(text)
                    if text.contains("original goal"))
                })
            }));
            assert!(requests[1..].iter().all(|request| {
                request.messages.iter().any(|message| {
                    matches!(message, sav::ModelMessage::User(text)
                    if text.contains("recovery") || text.contains("Recovery"))
                })
            }));
        }
    }
}

#[test]
fn generation_recovery_can_finish_without_spending_a_new_prompt_or_running_partial_tools() {
    let mut provider = Provider::repeating(150, false);
    provider.tools = true;
    provider.recover_after = Some(2);
    let executor = Executor::default();
    let result = runtime()
        .run(
            &mut session(),
            &provider,
            &executor,
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    assert!(result.success(), "{result:?}");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
    assert_eq!(executor.0.load(Ordering::SeqCst), 0);
    assert_eq!(result.tools_executed, 0);
}

#[test]
fn unary_length_repetition_does_not_double_the_response_allowance() {
    let mut provider = Provider::repeating(200, false);
    provider.unary = true;
    provider.length = true;
    provider.text = (0..480).map(|n| format!("word{} ", n % 200)).collect();
    let result = runtime()
        .run(
            &mut session(),
            &provider,
            &Executor::default(),
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    assert!(!result.success(), "{result:?}");
    assert!(result.failure.as_deref().unwrap().contains("repet"));
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request.max_output_tokens == Some(1024))
    );
}

#[test]
fn a_repetitive_attempt_stays_charged_against_the_same_token_budget() {
    let provider = Provider::repeating(150, false);
    let sink = Sink::default();
    let result = runtime()
        .with_limits(RunLimits {
            max_tokens: Some(2000),
            ..RunLimits::default()
        })
        .unwrap()
        .run(
            &mut session(),
            &provider,
            &Executor::default(),
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    assert!(result.budget_exhausted, "{result:?}");
    assert!(!result.success());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        final_progress(&sink),
        Some(Event::Progress {
            token_accounting: sav::runtime::TokenAccounting::Unavailable,
            ..
        })
    ));
}

#[test]
fn known_rejected_generation_usage_can_exceed_its_reservation_and_stays_charged() {
    let sink = Sink::default();
    let mut provider = Provider::repeating(150, false);
    provider.unary = true;
    provider.usage = Some(sav::ModelUsage {
        input_tokens: 10_000,
        output_tokens: 100,
        total_tokens: 10_100,
    });
    let result = runtime()
        .with_limits(RunLimits {
            max_tokens: Some(11_000),
            ..RunLimits::default()
        })
        .unwrap()
        .run(
            &mut session(),
            &provider,
            &Executor::default(),
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    assert!(result.budget_exhausted, "{result:?}");
    assert!(!result.success());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        final_progress(&sink),
        Some(Event::Progress {
            input_tokens: 10_000,
            output_tokens: 100,
            total_tokens: 10_100,
            token_accounting: sav::runtime::TokenAccounting::Provider,
            ..
        })
    ));
}

#[derive(Default)]
struct BrokenNoticeRecorder {
    fail_stream: bool,
    cancel_on_event: Option<CancellationToken>,
}
impl Recorder for BrokenNoticeRecorder {
    fn model_event(&mut self, _: &ModelStreamEvent) -> sav::runtime::Result<()> {
        if let Some(token) = &self.cancel_on_event {
            token.cancel();
        }
        if self.fail_stream {
            Err(sav::runtime::Error::Recording {
                reason: "stream evidence unavailable".into(),
            })
        } else {
            Ok(())
        }
    }
    fn notice(&mut self, message: &str) -> sav::runtime::Result<()> {
        if message.contains("repet") {
            Err(sav::runtime::Error::Recording {
                reason: "notice could not persist".into(),
            })
        } else {
            Ok(())
        }
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
fn required_recovery_recording_failure_stops_before_another_request_or_effect() {
    let provider = Provider::repeating(150, false);
    let executor = Executor::default();
    let result = runtime().run(
        &mut session(),
        &provider,
        &executor,
        &Sink::default(),
        &CancellationToken::new(),
        &mut ContextManager::new(32_768, 4),
        Some(&mut BrokenNoticeRecorder::default()),
    );
    assert!(matches!(result, Err(sav::runtime::Error::Recording { .. })));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(executor.0.load(Ordering::SeqCst), 0);
}

#[test]
fn required_stream_evidence_failure_preempts_recovery_and_preserves_the_prompt_token() {
    let provider = Provider::repeating(150, false);
    let executor = Executor::default();
    let token = CancellationToken::new();
    let result = runtime().run(
        &mut session(),
        &provider,
        &executor,
        &Sink::default(),
        &token,
        &mut ContextManager::new(32_768, 4),
        Some(&mut BrokenNoticeRecorder {
            fail_stream: true,
            ..Default::default()
        }),
    );
    assert!(matches!(result, Err(sav::runtime::Error::Recording { .. })));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(executor.0.load(Ordering::SeqCst), 0);
    assert!(!token.is_cancelled());
}

#[test]
fn cancellation_during_required_recording_preempts_presentation_and_recovery() {
    let provider = Provider::repeating(150, false);
    let token = CancellationToken::new();
    let sink = Sink::default();
    let result = runtime()
        .run(
            &mut session(),
            &provider,
            &Executor::default(),
            &sink,
            &token,
            &mut ContextManager::new(32_768, 4),
            Some(&mut BrokenNoticeRecorder {
                cancel_on_event: Some(token.clone()),
                ..Default::default()
            }),
        )
        .unwrap();
    assert!(result.cancelled, "{result:?}");
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

#[test]
fn explicit_intentional_repetition_exception_accepts_output_but_not_repeated_tools() {
    let provider = Provider::repeating(100, false);
    let result = runtime()
        .with_generation_loop_detection(false)
        .run(
            &mut session(),
            &provider,
            &Executor::default(),
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    assert!(result.success(), "{result:?}");
    assert_eq!(result.answer.as_deref(), Some(provider.text.as_str()));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    let mut provider = Provider::repeating(100, false);
    provider.tools = true;
    let executor = Executor::default();
    let result = runtime()
        .with_generation_loop_detection(false)
        .run(
            &mut session(),
            &provider,
            &executor,
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    assert!(!result.success(), "{result:?}");
    assert!(
        result
            .failure
            .as_deref()
            .unwrap()
            .to_lowercase()
            .contains("repeated")
    );
    assert!(executor.0.load(Ordering::SeqCst) <= 2);
    assert!(provider.calls.load(Ordering::SeqCst) > executor.0.load(Ordering::SeqCst));
}

struct CancelSink(CancellationToken);
impl EventSink for CancelSink {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        if matches!(event, Event::Text(_)) {
            self.0.cancel();
        }
        Ok(())
    }
}

#[test]
fn user_cancellation_is_not_relabelled_as_generation_failure_or_retried() {
    let provider = Provider::repeating(150, false);
    let token = CancellationToken::new();
    let result = runtime()
        .run(
            &mut session(),
            &provider,
            &Executor::default(),
            &CancelSink(token.clone()),
            &token,
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    assert!(result.cancelled, "{result:?}");
    assert!(!result.budget_exhausted);
    assert!(result.failure.is_none());
    assert!(result.answer.is_none());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

struct GuidanceSink {
    controls: sav::runtime::ControlMailbox,
    sent: std::sync::atomic::AtomicBool,
}
impl EventSink for GuidanceSink {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        if matches!(event, Event::Text(_)) && !self.sent.swap(true, Ordering::SeqCst) {
            self.controls.steer("Use the supplied human correction.")?;
        }
        Ok(())
    }
}

#[test]
fn human_guidance_preempts_the_repetition_diagnosis_without_an_autonomous_correction() {
    let mut provider = Provider::repeating(150, false);
    provider.recover_after = Some(1);
    let controls = sav::runtime::ControlMailbox::default();
    let sink = GuidanceSink {
        controls: controls.clone(),
        sent: std::sync::atomic::AtomicBool::new(false),
    };
    let token = CancellationToken::new();
    let result = runtime()
        .run_controlled(
            &mut session(),
            &provider,
            &Executor::default(),
            &sink,
            &token,
            &mut ContextManager::new(32_768, 4),
            None,
            &controls,
        )
        .unwrap();
    assert!(result.success(), "{result:?}");
    assert!(!token.is_cancelled());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    let requests = provider.requests.lock().unwrap();
    assert!(requests[1].messages.iter().any(|message| matches!(message,
        sav::ModelMessage::User(text) if text == "Use the supplied human correction."
    )));
    assert!(!requests[1].messages.iter().any(|message| matches!(message,
        sav::ModelMessage::User(text) if text.starts_with("Detected repetitive generation")
    )));
}

struct AlternatingProvider(Provider);
impl ModelTransport for AlternatingProvider {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!("fixture streams")
    }
    fn stream(
        &self,
        request: &ModelRequest,
        _: &CancellationToken,
        emit: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        self.0.requests.lock().unwrap().push(request.clone());
        let call = self.0.calls.fetch_add(1, Ordering::SeqCst);
        if call.is_multiple_of(2) {
            emit(ModelStreamEvent::TextDelta(self.0.text.clone()))?;
            unreachable!("the repeated generation is interrupted")
        }
        let mut progress = turn(
            (0..700).map(|n| format!("detail{call}_{n} ")).collect(),
            None,
            true,
            false,
        );
        progress.tool_intents[0].arguments = serde_json::json!({"index":call});
        Ok(progress)
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}

#[test]
fn completed_effects_and_compaction_do_not_replenish_generation_corrections() {
    let provider = AlternatingProvider(Provider::repeating(150, false));
    let executor = Executor::default();
    let mut context = ContextManager::new(5000, 1);
    let token = CancellationToken::new();
    let result = runtime()
        .run(
            &mut session(),
            &provider,
            &executor,
            &Sink::default(),
            &token,
            &mut context,
            None,
        )
        .unwrap();
    assert!(!result.success(), "{result:?}");
    assert!(
        result
            .failure
            .as_deref()
            .unwrap()
            .contains("two autonomous corrections")
    );
    assert_eq!(provider.0.calls.load(Ordering::SeqCst), 5);
    assert_eq!(executor.0.load(Ordering::SeqCst), 2);
    assert_eq!(result.tools_executed, 2);
    assert!(context.compactions() > 0);
    assert!(!token.is_cancelled());
}

struct LateRecoveryGuidance {
    controls: sav::runtime::ControlMailbox,
    sent: std::sync::atomic::AtomicBool,
}
impl EventSink for LateRecoveryGuidance {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        if matches!(event, Event::Note(ref text) if text.contains("Recovery "))
            && !self.sent.swap(true, Ordering::SeqCst)
        {
            self.controls
                .steer("Human help arriving alongside recovery.")?;
        }
        Ok(())
    }
}

#[test]
fn guidance_during_recovery_recording_does_not_spend_an_undelivered_correction() {
    let provider = Provider::repeating(150, false);
    let controls = sav::runtime::ControlMailbox::default();
    let sink = LateRecoveryGuidance {
        controls: controls.clone(),
        sent: std::sync::atomic::AtomicBool::new(false),
    };
    let result = runtime()
        .run_controlled(
            &mut session(),
            &provider,
            &Executor::default(),
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
            &controls,
        )
        .unwrap();
    assert!(!result.success());
    let requests = provider.requests.lock().unwrap();
    for number in [1, 2] {
        assert!(
            requests
                .iter()
                .any(|request| request.messages.iter().any(|message| {
                    matches!(message, sav::ModelMessage::User(text)
                if text.contains(&format!("Recovery {number}/2")))
                })),
            "the model must receive correction {number}"
        );
    }
}

enum ReviewScenario {
    SteeredKnownUsage(sav::runtime::ControlMailbox),
    LengthThenKnown,
    FailedThenKnown,
}
struct ReviewProvider {
    scenario: ReviewScenario,
    calls: AtomicUsize,
}
impl ReviewProvider {
    fn new(scenario: ReviewScenario) -> Self {
        Self {
            scenario,
            calls: AtomicUsize::new(0),
        }
    }
}
impl ModelTransport for ReviewProvider {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!("fixture streams")
    }
    fn stream(
        &self,
        _: &ModelRequest,
        _: &CancellationToken,
        _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let mut response = turn("Supported response.".into(), None, false, false);
        response.usage = Some(sav::ModelUsage {
            input_tokens: 12,
            output_tokens: 6,
            total_tokens: 18,
        });
        if call == 0 {
            match &self.scenario {
                ReviewScenario::SteeredKnownUsage(controls) => {
                    response.usage = Some(sav::ModelUsage {
                        input_tokens: 10_000,
                        output_tokens: 100,
                        total_tokens: 10_100,
                    });
                    controls
                        .steer("Use the supplied new human guidance.")
                        .expect("active fixture");
                }
                ReviewScenario::LengthThenKnown => {
                    response.finish_reason = FinishReason::Length;
                    response.usage = Some(sav::ModelUsage {
                        input_tokens: 20,
                        output_tokens: 8,
                        total_tokens: 28,
                    });
                }
                ReviewScenario::FailedThenKnown => return Err(sav::Error::ModelTransportTimedOut),
            }
        }
        Ok(response)
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}

#[test]
fn completed_response_usage_is_charged_even_when_human_steering_wins() {
    let controls = sav::runtime::ControlMailbox::default();
    let provider = ReviewProvider::new(ReviewScenario::SteeredKnownUsage(controls.clone()));
    let result = runtime()
        .with_limits(RunLimits {
            max_tokens: Some(11_000),
            ..RunLimits::default()
        })
        .unwrap()
        .run_controlled(
            &mut session(),
            &provider,
            &Executor::default(),
            &Sink::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
            &controls,
        )
        .unwrap();
    assert!(result.budget_exhausted, "{result:?}");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn every_completed_length_attempt_contributes_to_provider_progress_totals() {
    let provider = ReviewProvider::new(ReviewScenario::LengthThenKnown);
    let sink = Sink::default();
    let result = runtime()
        .run(
            &mut session(),
            &provider,
            &Executor::default(),
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    assert!(result.success(), "{result:?}");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert!(matches!(
        final_progress(&sink),
        Some(Event::Progress {
            input_tokens: 32,
            output_tokens: 14,
            total_tokens: 46,
            token_accounting: sav::runtime::TokenAccounting::Provider,
            ..
        })
    ));
}

#[test]
fn unknown_failed_attempt_usage_does_not_become_complete_provider_accounting() {
    let provider = ReviewProvider::new(ReviewScenario::FailedThenKnown);
    let sink = Sink::default();
    let result = runtime()
        .run(
            &mut session(),
            &provider,
            &Executor::default(),
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    assert!(result.success(), "{result:?}");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert!(matches!(
        final_progress(&sink),
        Some(Event::Progress {
            input_tokens: 12,
            output_tokens: 6,
            total_tokens: 18,
            token_accounting: sav::runtime::TokenAccounting::Unavailable,
            ..
        })
    ));
}

struct CompletedAfterRejection(Provider);
impl ModelTransport for CompletedAfterRejection {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!("fixture streams")
    }
    fn stream(
        &self,
        _: &ModelRequest,
        _: &CancellationToken,
        emit: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        assert!(emit(ModelStreamEvent::TextDelta(self.0.text.clone())).is_err());
        let mut response = turn("Different completed text.".into(), None, true, false);
        response.usage = Some(sav::ModelUsage {
            input_tokens: 10_000,
            output_tokens: 100,
            total_tokens: 10_100,
        });
        Ok(response)
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}

#[test]
fn completed_transport_cannot_erase_rejection_or_known_usage_by_ignoring_callback() {
    let provider = CompletedAfterRejection(Provider::repeating(150, false));
    let executor = Executor::default();
    let sink = Sink::default();
    let result = runtime()
        .with_limits(RunLimits {
            max_tokens: Some(11_000),
            ..RunLimits::default()
        })
        .unwrap()
        .run(
            &mut session(),
            &provider,
            &executor,
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(32_768, 4),
            None,
        )
        .unwrap();
    assert!(result.budget_exhausted, "{result:?}");
    assert!(!result.success());
    assert_eq!(provider.0.calls.load(Ordering::SeqCst), 1);
    assert_eq!(executor.0.load(Ordering::SeqCst), 0);
    assert!(matches!(
        final_progress(&sink),
        Some(Event::Progress {
            input_tokens: 10_000,
            output_tokens: 100,
            total_tokens: 10_100,
            token_accounting: sav::runtime::TokenAccounting::Provider,
            ..
        })
    ));
}
