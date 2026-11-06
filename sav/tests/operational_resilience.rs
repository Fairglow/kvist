use sav::runtime::{
    AgentSession, ContextManager, ControlMailbox, Event, EventSink, GuidanceState, ModelIdentity,
    NativeRuntime, Recorder, RetryPolicy, RunLimits, RunSummary, ToolExecutor, ToolOutcome,
};
use sav::{
    CancellationToken, FinishReason, LocalModelProvider, ModelRequest, ModelStreamEvent,
    ModelTransport, ModelTurn, ModelUsage, ReasoningEffort, ToolIntent,
};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Clone, Copy)]
enum Mode {
    Answer,
    Tools,
    BrokenStream,
    CancelWithGuidance,
    GuidanceBatch,
    Deadline,
    WithdrawnInterruption,
    ContextBound,
    TerminalFailure,
    GenerationStall,
    ToolStall,
    CancelWithAccountingFailure,
    TerminalFailureWithTools,
}

struct Provider {
    mode: Mode,
    controls: ControlMailbox,
    parent: CancellationToken,
    calls: AtomicUsize,
}

impl ModelTransport for Provider {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!("fixture exercises the stream boundary")
    }

    fn stream(
        &self,
        _: &ModelRequest,
        _: &CancellationToken,
        emit: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        match self.mode {
            Mode::BrokenStream => {
                for _ in 0..3 {
                    assert!(emit(ModelStreamEvent::TextDelta("provisional".into())).is_err());
                }
            }
            Mode::CancelWithGuidance => {
                self.controls.steer("undelivered").unwrap();
                self.parent.cancel();
                return Err(sav::Error::ModelTransportCancelled);
            }
            Mode::GuidanceBatch => {
                for index in 0..3 {
                    self.controls.steer(format!("guidance {index}")).unwrap();
                }
                return Err(sav::Error::ModelTransportCancelled);
            }
            Mode::Deadline => {
                let until = std::time::Instant::now() + Duration::from_secs(1);
                while !self.parent.is_cancelled() && std::time::Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(
                    self.parent.is_cancelled(),
                    "owned deadline watcher must stop inference"
                );
                return Err(sav::Error::ModelTransportCancelled);
            }
            Mode::WithdrawnInterruption => {
                let id = self.controls.steer("withdraw after interruption").unwrap();
                assert!(emit(ModelStreamEvent::TextDelta("interrupted".into())).is_err());
                self.controls.withdraw(id).unwrap();
                return Err(sav::Error::ModelTransportCancelled);
            }
            Mode::GenerationStall => {
                let passage = (0..100)
                    .map(|word| format!("word{word} "))
                    .collect::<String>();
                assert!(emit(ModelStreamEvent::TextDelta(passage.repeat(4))).is_err());
            }
            Mode::ToolStall => {}
            Mode::CancelWithAccountingFailure => {
                self.parent.cancel();
                return Err(sav::Error::ModelTransportCancelled);
            }
            Mode::Answer
            | Mode::Tools
            | Mode::ContextBound
            | Mode::TerminalFailure
            | Mode::TerminalFailureWithTools => {}
        }
        let tools = if matches!(self.mode, Mode::ToolStall) {
            vec![ToolIntent {
                id: format!("effect-{call}"),
                provider_id: None,
                name: "effect".into(),
                arguments: serde_json::json!({"path":0}),
            }]
        } else if matches!(
            self.mode,
            Mode::Tools | Mode::BrokenStream | Mode::TerminalFailureWithTools
        ) && call == 0
        {
            (0..2)
                .map(|index| ToolIntent {
                    id: format!("effect-{index}"),
                    provider_id: None,
                    name: "effect".into(),
                    arguments: serde_json::json!({"path":index}),
                })
                .collect()
        } else {
            vec![]
        };
        Ok(ModelTurn {
            text: "Finished the goal.".into(),
            reasoning: None,
            finish_reason: if tools.is_empty() {
                FinishReason::Stop
            } else {
                FinishReason::ToolCalls
            },
            tool_intents: tools,
            provider: LocalModelProvider::Ollama,
            model: "fixture".into(),
            response_id: None,
            provider_request_id: None,
            usage: Some(ModelUsage {
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
            }),
        })
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(1)
    }
}

#[derive(Default)]
struct Sink(Mutex<Vec<Event>>, bool, bool);

impl EventSink for Sink {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        let terminal = matches!(
            event,
            Event::Finished { .. } | Event::Failed(_) | Event::PromptEnd { .. }
        );
        let progress = matches!(event, Event::Progress { .. });
        self.0.lock().unwrap().push(event);
        if terminal && self.1 || progress && self.2 {
            Err(sav::runtime::Error::ChannelClosed)
        } else {
            Ok(())
        }
    }
}

struct NoEffects;

impl ToolExecutor for NoEffects {
    fn execute(&self, _: &ToolIntent, _: &CancellationToken) -> sav::runtime::Result<ToolOutcome> {
        panic!("this fixture must not dispatch effects")
    }
}

#[derive(Default)]
struct Record {
    broken_stream: bool,
    broken_start: bool,
    broken_prompt: bool,
    broken_finish: bool,
    broken_tool: bool,
    broken_diagnostic: bool,
    broken_delivered: bool,
    broken_stall_notice: bool,
    slow_drop: bool,
    complete_turns: usize,
    summaries: Vec<RunSummary>,
    outcomes: usize,
    model_events: usize,
}

fn recording_error() -> sav::runtime::Error {
    sav::runtime::Error::Recording {
        reason: "injected required-record failure".into(),
    }
}

impl Recorder for Record {
    fn notice(&mut self, message: &str) -> sav::runtime::Result<()> {
        if self.broken_stall_notice && message.contains("after two autonomous corrections") {
            Err(recording_error())
        } else {
            Ok(())
        }
    }
    fn model_event(&mut self, _: &ModelStreamEvent) -> sav::runtime::Result<()> {
        self.model_events += 1;
        if self.broken_stream {
            Err(recording_error())
        } else {
            Ok(())
        }
    }
    fn control(&mut self, notice: &sav::runtime::GuidanceNotice) -> sav::runtime::Result<()> {
        if self.broken_delivered && notice.state == GuidanceState::Delivered {
            return Err(recording_error());
        }
        if self.slow_drop && notice.state == GuidanceState::Dropped {
            std::thread::sleep(Duration::from_millis(150));
        }
        Ok(())
    }
    fn on_prompt(&mut self, _: &str) -> sav::runtime::Result<()> {
        if self.broken_prompt {
            Err(recording_error())
        } else {
            Ok(())
        }
    }
    fn session_start(&mut self) -> sav::runtime::Result<()> {
        if self.broken_start {
            Err(recording_error())
        } else {
            Ok(())
        }
    }
    fn request(&mut self, _: &ModelRequest, _: u32) -> sav::runtime::Result<()> {
        Ok(())
    }
    fn turn_start(&mut self, _: &ModelTurn) -> sav::runtime::Result<usize> {
        self.complete_turns += 1;
        Ok(self.complete_turns)
    }
    fn turn_finish(
        &mut self,
        _: usize,
        _: &Option<ModelUsage>,
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
        self.outcomes += 1;
        if self.broken_tool {
            Err(recording_error())
        } else {
            Ok(())
        }
    }
    fn tool_failure(&mut self, _: &sav::runtime::ToolFailure) -> sav::runtime::Result<()> {
        if self.broken_diagnostic {
            Err(recording_error())
        } else {
            Ok(())
        }
    }
    fn session_finish(&mut self, summary: &RunSummary) -> sav::runtime::Result<()> {
        self.summaries.push(summary.clone());
        if self.broken_finish {
            Err(recording_error())
        } else {
            Ok(())
        }
    }
}

fn run(
    mode: Mode,
    record: &mut Record,
    wall_time: Option<Duration>,
) -> (sav::runtime::Result<RunSummary>, Sink, Provider) {
    run_custom(
        mode,
        record,
        RunLimits {
            wall_time,
            ..RunLimits::default()
        },
        &NoEffects,
        None,
    )
}

fn run_custom<E: ToolExecutor>(
    mode: Mode,
    record: &mut Record,
    limits: RunLimits,
    executor: &E,
    max_turns: Option<u32>,
) -> (sav::runtime::Result<RunSummary>, Sink, Provider) {
    let parent = CancellationToken::new();
    let controls = ControlMailbox::default();
    let provider = Provider {
        mode,
        controls: controls.clone(),
        parent: parent.clone(),
        calls: AtomicUsize::new(0),
    };
    let mut session = AgentSession::new(
        ModelIdentity {
            id: "fixture".into(),
            model: "fixture".into(),
        },
        ReasoningEffort::None,
        vec![],
        "Host owns all authority.".into(),
    );
    session.push_user(if matches!(mode, Mode::ContextBound) {
        "original ".repeat(6000)
    } else {
        "Original goal.".into()
    });
    let sink = Sink(
        Mutex::new(vec![]),
        matches!(mode, Mode::TerminalFailure | Mode::TerminalFailureWithTools),
        matches!(mode, Mode::CancelWithAccountingFailure),
    );
    let result = NativeRuntime::with_retry(
        max_turns,
        RetryPolicy::new(1, Duration::ZERO, Duration::ZERO),
    )
    .with_limits(limits)
    .unwrap()
    .run_controlled(
        &mut session,
        &provider,
        executor,
        &sink,
        &parent,
        &mut ContextManager::new(8192, 4),
        Some(record),
        &controls,
    );
    (result, sink, provider)
}

#[test]
fn error_return_still_presents_terminal_failure_without_inventing_a_turn() {
    let mut record = Record {
        broken_stream: true,
        ..Record::default()
    };
    let (result, sink, provider) = run(Mode::BrokenStream, &mut record, None);
    assert!(result.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(record.complete_turns, 0);
    assert_eq!(record.model_events, 1);
    assert_eq!(record.summaries.len(), 1);
    assert!(matches!(
        sink.0.lock().unwrap().last(),
        Some(Event::Failed(_))
    ));
}

#[test]
fn error_return_flushes_known_usage_even_when_provider_ignores_failed_callback() {
    let mut record = Record {
        broken_stream: true,
        ..Record::default()
    };
    let (result, sink, _) = run(Mode::BrokenStream, &mut record, None);
    assert!(result.is_err());
    let events = sink.0.lock().unwrap();
    let progress = events
        .iter()
        .rev()
        .find(|event| matches!(event, Event::Progress { .. }));
    assert!(matches!(
        progress,
        Some(Event::Progress {
            token_accounting: sav::runtime::TokenAccounting::Provider,
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
            ..
        })
    ));
    assert_eq!(record.complete_turns, 0);
}

#[test]
fn cleanup_deadline_cannot_replace_prior_user_cancellation() {
    let mut record = Record {
        slow_drop: true,
        ..Record::default()
    };
    let (result, _, _) = run(
        Mode::CancelWithGuidance,
        &mut record,
        Some(Duration::from_millis(100)),
    );
    let result = result.unwrap();
    assert!(result.cancelled, "{result:?}");
    assert!(!result.budget_exhausted, "{result:?}");
    assert_eq!(
        result.stop_reason,
        Some(sav::runtime::StopReason::Cancelled)
    );
    assert!(!result.success());
}

#[test]
fn claimed_guidance_batch_receives_final_drop_states_after_recording_failure() {
    let mut record = Record {
        broken_prompt: true,
        ..Record::default()
    };
    let (result, sink, provider) = run(Mode::GuidanceBatch, &mut record, None);
    assert!(result.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let events = sink.0.lock().unwrap();
    let dropped: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Control(notice) if notice.state == GuidanceState::Dropped => Some(notice.id.0),
            _ => None,
        })
        .collect();
    assert_eq!(dropped.len(), 3, "{events:?}");
    assert_eq!(
        dropped
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
}

#[test]
fn terminal_record_failure_is_presented_instead_of_success_or_silence() {
    let mut record = Record {
        broken_finish: true,
        ..Record::default()
    };
    let (result, sink, _) = run(Mode::Answer, &mut record, None);
    assert!(result.is_err());
    let events = sink.0.lock().unwrap();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::Finished { .. }))
    );
    assert!(matches!(events.last(), Some(Event::Failed(text)) if text.contains("required-record")));
}

struct Executor {
    outcome: ToolOutcome,
    failure: Option<sav::runtime::ToolFailureCategory>,
    progress_error: bool,
    calls: AtomicUsize,
}

impl Executor {
    fn new(status: Option<i32>) -> Self {
        Self {
            outcome: ToolOutcome {
                exited: status.is_some(),
                status,
                stdout: vec![],
                stderr: b"actionable fixture diagnostics".to_vec(),
                timed_out: false,
                output_limit_exceeded: false,
                cancelled: false,
            },
            failure: None,
            progress_error: false,
            calls: AtomicUsize::new(0),
        }
    }
}

impl ToolExecutor for Executor {
    fn execute(&self, _: &ToolIntent, _: &CancellationToken) -> sav::runtime::Result<ToolOutcome> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        use sav::runtime::{Error, ToolFailureCategory as Category};
        let error = match self.failure {
            Some(Category::Denied) => Some(Error::ToolPolicy {
                tool: "effect".into(),
                reason: "grant refused".into(),
            }),
            Some(Category::InvalidRequest) => Some(Error::ToolRender {
                tool: "effect".into(),
                reason: "invalid arguments".into(),
            }),
            Some(Category::Prerequisite) => Some(Error::ToolPrerequisite {
                tool: "effect".into(),
                reason: "approved local runner is absent".into(),
            }),
            Some(Category::Cancelled) => Some(Error::Cancelled),
            Some(_) => Some(Error::Host {
                reason: "executor failed after possible effects".into(),
            }),
            None => None,
        };
        error.map_or_else(|| Ok(self.outcome.clone()), Err)
    }
    fn progress(
        &self,
        _: &ToolIntent,
        _: &ToolOutcome,
    ) -> sav::runtime::Result<sav::loop_detection::ProgressEvidence> {
        if self.progress_error {
            Err(sav::runtime::Error::Host {
                reason: "post-dispatch revision unavailable".into(),
            })
        } else {
            Ok(sav::loop_detection::ProgressEvidence::None)
        }
    }
}

#[test]
fn failure_categories_preserve_uncertainty_and_bound_printable_diagnostics() {
    use sav::runtime::{
        EffectCertainty, RecoveryAction, ToolFailure, ToolFailureCategory as Category,
    };
    let mut outcome = Executor::new(Some(1)).outcome;
    for (category, flag) in [
        (Category::ProcessExit, 0),
        (Category::Timeout, 1),
        (Category::OutputLimit, 2),
        (Category::Cancelled, 3),
        (Category::Unobserved, 4),
    ] {
        outcome.timed_out = flag == 1;
        outcome.output_limit_exceeded = flag == 2;
        outcome.cancelled = flag == 3;
        outcome.status = if flag == 4 { None } else { Some(1) };
        outcome.stderr = b"\x1b]52;hostile\x07".repeat(2000);
        let failure = ToolFailure::from_outcome(&outcome).unwrap();
        assert_eq!(failure.category, category);
        assert_eq!(failure.effects, EffectCertainty::MayHaveEffects);
        assert!(failure.message.len() <= 4096);
        assert!(!failure.message.contains('\x1b'));
        assert!(!failure.message.contains('\x07'));
        assert!(!failure.next_step.is_empty());
        if flag == 4 {
            assert_eq!(failure.recovery, RecoveryAction::StopAndReconcile);
            assert!(failure.next_step.contains("does not establish OOM"));
        }
    }
    assert!(ToolFailure::from_outcome(&Executor::new(Some(0)).outcome).is_none());
}

#[test]
fn dispatched_failure_and_known_refusal_have_different_effect_certainty() {
    use sav::runtime::{EffectCertainty, ToolFailureCategory as Category};
    for category in [
        Category::Denied,
        Category::InvalidRequest,
        Category::Prerequisite,
        Category::Unobserved,
    ] {
        let mut executor = Executor::new(Some(0));
        executor.failure = Some(category);
        let (result, sink, _) = run_custom(
            Mode::Tools,
            &mut Record::default(),
            RunLimits::default(),
            &executor,
            None,
        );
        let events = sink.0.lock().unwrap();
        let failure = events
            .iter()
            .find_map(|event| match event {
                Event::ToolFailure { failure, .. } => Some(failure),
                _ => None,
            })
            .unwrap();
        assert_eq!(failure.category, category);
        assert_eq!(
            failure.effects,
            if category == Category::Unobserved {
                EffectCertainty::MayHaveEffects
            } else {
                EffectCertainty::NotExecuted
            }
        );
        if matches!(category, Category::Unobserved | Category::Prerequisite) {
            assert!(!result.unwrap().success());
            assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
        }
    }
}

#[test]
fn failed_required_tool_diagnostic_stops_remaining_effects_and_presents_cause() {
    let executor = Executor::new(Some(1));
    let mut record = Record {
        broken_diagnostic: true,
        ..Record::default()
    };
    let (result, sink, provider) = run_custom(
        Mode::Tools,
        &mut record,
        RunLimits::default(),
        &executor,
        None,
    );
    assert!(result.is_err());
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(record.outcomes, 1);
    let events = sink.0.lock().unwrap();
    assert!(events.iter().any(|event| matches!(event, Event::ToolFailure {failure,..} if failure.message.contains("actionable fixture"))));
    assert!(matches!(events.last(), Some(Event::Failed(_))));
}

#[test]
fn post_dispatch_observation_failure_preserves_outcome_before_uncertainty() {
    let mut executor = Executor::new(Some(0));
    executor.progress_error = true;
    let mut record = Record::default();
    let (result, sink, _) = run_custom(
        Mode::Tools,
        &mut record,
        RunLimits::default(),
        &executor,
        None,
    );
    assert!(result.is_err());
    assert_eq!(record.outcomes, 1);
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
    let events = sink.0.lock().unwrap();
    let outcome = events
        .iter()
        .position(|event| matches!(event, Event::ToolResult { .. }))
        .unwrap();
    let failure = events.iter().position(|event| matches!(event, Event::ToolFailure {failure,..} if failure.effects == sav::runtime::EffectCertainty::MayHaveEffects)).unwrap();
    assert!(outcome < failure);
    assert!(matches!(events.last(), Some(Event::Failed(_))));
}

#[test]
fn token_and_turn_limits_keep_distinct_structured_resource_identity() {
    use sav::runtime::{BudgetResource, StopReason};
    for resource in [BudgetResource::ModelTokens, BudgetResource::Turns] {
        let mut record = Record::default();
        let limits = RunLimits {
            max_tokens: (resource == BudgetResource::ModelTokens).then_some(1),
            ..RunLimits::default()
        };
        let (result, sink, _) = run_custom(
            Mode::Tools,
            &mut record,
            limits,
            &Executor::new(Some(0)),
            Some(1),
        );
        let summary = result.unwrap();
        assert_eq!(summary.stop_reason, Some(StopReason::Limit(resource)));
        assert!(!summary.success());
        assert!(
            matches!(sink.0.lock().unwrap().last(), Some(Event::PromptEnd {reason:StopReason::Limit(actual), ..}) if *actual == resource)
        );
        assert_eq!(
            serde_json::to_value(StopReason::Limit(resource)).unwrap()["kind"],
            "limit"
        );
    }
}

#[test]
fn claimed_delivered_guidance_stays_delivered_when_its_recording_fails() {
    let mut record = Record {
        broken_delivered: true,
        ..Record::default()
    };
    let (result, sink, provider) = run(Mode::GuidanceBatch, &mut record, None);
    assert!(result.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let events = sink.0.lock().unwrap();
    assert!(events.iter().any(|event| matches!(event, Event::Control(notice) if notice.id.0 == 1 && notice.state == GuidanceState::Delivered)));
    assert!(!events.iter().any(|event| matches!(event, Event::Control(notice) if notice.id.0 == 1 && notice.state == GuidanceState::Dropped)));
    for id in [2, 3] {
        assert!(events.iter().any(|event| matches!(event, Event::Control(notice) if notice.id.0 == id && notice.state == GuidanceState::Dropped)));
    }
}

#[test]
fn terminal_record_failure_preserves_original_hard_executor_cause() {
    let mut executor = Executor::new(Some(0));
    executor.failure = Some(sav::runtime::ToolFailureCategory::Unobserved);
    let mut record = Record {
        broken_finish: true,
        ..Record::default()
    };
    let (result, _, _) = run_custom(
        Mode::Tools,
        &mut record,
        RunLimits::default(),
        &executor,
        None,
    );
    let error = result.unwrap_err().describe();
    assert!(
        error.contains("executor failed after possible effects"),
        "{error}"
    );
    assert!(error.contains("required-record"), "{error}");
}

#[test]
fn executor_cancellation_is_not_an_unobserved_host_failure() {
    let mut executor = Executor::new(Some(0));
    executor.failure = Some(sav::runtime::ToolFailureCategory::Cancelled);
    let (result, sink, _) = run_custom(
        Mode::Tools,
        &mut Record::default(),
        RunLimits::default(),
        &executor,
        None,
    );
    let summary = result.unwrap();
    assert_eq!(
        summary.stop_reason,
        Some(sav::runtime::StopReason::Cancelled),
        "{summary:?}"
    );
    assert!(summary.cancelled);
    let events = sink.0.lock().unwrap();
    assert!(events.iter().any(|event| matches!(event,
        Event::ToolFailure {failure,..}
        if failure.category == sav::runtime::ToolFailureCategory::Cancelled
        && failure.effects == sav::runtime::EffectCertainty::MayHaveEffects
    )));
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn wall_context_and_attempt_bounds_remain_typed_through_distinct_exit_paths() {
    use sav::runtime::{BudgetResource, StopReason};
    for (mode, resource, wall_time, reserve, encoded) in [
        (
            Mode::Deadline,
            BudgetResource::WallTime,
            Some(Duration::from_millis(50)),
            1024,
            "wall_time",
        ),
        (
            Mode::ContextBound,
            BudgetResource::Context,
            None,
            1024,
            "context",
        ),
        (
            Mode::WithdrawnInterruption,
            BudgetResource::ModelAttempts,
            None,
            1024,
            "model_attempts",
        ),
    ] {
        let mut record = Record::default();
        let (result, sink, _) = run_custom(
            mode,
            &mut record,
            RunLimits {
                wall_time,
                response_reserve: reserve,
                ..RunLimits::default()
            },
            &NoEffects,
            None,
        );
        assert_eq!(
            record.summaries.last().unwrap().stop_reason,
            Some(StopReason::Limit(resource))
        );
        assert!(!record.summaries.last().unwrap().success());
        if resource == BudgetResource::WallTime {
            assert_eq!(
                result.unwrap().stop_reason,
                Some(StopReason::Limit(resource))
            );
            assert!(
                matches!(sink.0.lock().unwrap().last(), Some(Event::PromptEnd {reason:StopReason::Limit(actual),..}) if *actual == resource)
            );
        } else {
            assert!(result.is_err());
            assert!(matches!(
                sink.0.lock().unwrap().last(),
                Some(Event::Failed(_))
            ));
        }
        assert_eq!(
            serde_json::to_value(StopReason::Limit(resource)).unwrap(),
            serde_json::json!({"kind":"limit","resource":encoded}),
        );
    }
}

#[test]
fn hard_executor_cause_survives_failed_outcome_recording() {
    let mut executor = Executor::new(Some(0));
    executor.failure = Some(sav::runtime::ToolFailureCategory::Unobserved);
    let mut record = Record {
        broken_tool: true,
        ..Record::default()
    };
    let (result, _, _) = run_custom(
        Mode::Tools,
        &mut record,
        RunLimits::default(),
        &executor,
        None,
    );
    let error = result.unwrap_err().describe();
    assert!(
        error.contains("executor failed after possible effects"),
        "{error}"
    );
    assert!(error.contains("required-record"), "{error}");
    assert_eq!(executor.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn progress_observation_cause_survives_failed_diagnostic_recording() {
    let mut executor = Executor::new(Some(0));
    executor.progress_error = true;
    let mut record = Record {
        broken_diagnostic: true,
        ..Record::default()
    };
    let (result, _, _) = run_custom(
        Mode::Tools,
        &mut record,
        RunLimits::default(),
        &executor,
        None,
    );
    let error = result.unwrap_err().describe();
    assert!(
        error.contains("post-dispatch revision unavailable"),
        "{error}"
    );
    assert!(error.contains("required-record"), "{error}");
    assert_eq!(record.outcomes, 1);
}

#[test]
fn executor_cancellation_survives_failed_outcome_recording() {
    let mut executor = Executor::new(Some(0));
    executor.failure = Some(sav::runtime::ToolFailureCategory::Cancelled);
    let mut record = Record {
        broken_tool: true,
        ..Record::default()
    };
    let (result, _, _) = run_custom(
        Mode::Tools,
        &mut record,
        RunLimits::default(),
        &executor,
        None,
    );
    assert!(result.is_err());
    let summary = record.summaries.last().unwrap();
    assert_eq!(
        summary.stop_reason,
        Some(sav::runtime::StopReason::Cancelled),
        "{summary:?}"
    );
    assert!(summary.cancelled);
    assert!(!summary.budget_exhausted);
    assert!(!summary.success());
}

#[test]
fn generic_io_does_not_claim_a_predispatch_prerequisite_failure() {
    let failure = sav::runtime::ToolFailure::from_error(&sav::runtime::Error::Io(
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    ));
    assert_eq!(
        failure.category,
        sav::runtime::ToolFailureCategory::Unobserved
    );
    assert_eq!(
        failure.effects,
        sav::runtime::EffectCertainty::MayHaveEffects
    );
}

#[test]
fn failed_session_start_still_attempts_terminal_state_without_inference() {
    let mut record = Record {
        broken_start: true,
        ..Record::default()
    };
    let (result, sink, provider) = run(Mode::Answer, &mut record, None);
    assert!(result.is_err());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(record.summaries.len(), 1);
    assert_eq!(
        record.summaries[0].stop_reason,
        Some(sav::runtime::StopReason::Failed)
    );
    assert!(matches!(
        sink.0.lock().unwrap().last(),
        Some(Event::Failed(_))
    ));
}

#[test]
fn failed_terminal_presentation_returns_error_after_one_record_attempt() {
    let mut record = Record::default();
    let (result, _, provider) = run(Mode::TerminalFailure, &mut record, None);
    assert!(matches!(result, Err(sav::runtime::Error::ChannelClosed)));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(record.summaries.len(), 1);
    assert!(record.summaries[0].success());
}

fn assert_stall_notice_cause(mode: Mode) {
    let mut record = Record {
        broken_stall_notice: true,
        ..Record::default()
    };
    let (result, _, _) = run_custom(
        mode,
        &mut record,
        RunLimits::default(),
        &Executor::new(Some(0)),
        None,
    );
    let error = result.unwrap_err().describe();
    assert!(error.contains("stalled"), "{error}");
    assert!(error.contains("required-record"), "{error}");
    assert_eq!(
        record.summaries.last().unwrap().stop_reason,
        Some(sav::runtime::StopReason::Stalled)
    );
}

#[test]
fn stalled_generation_cause_survives_failed_exhaustion_notice() {
    assert_stall_notice_cause(Mode::GenerationStall);
}

#[test]
fn stalled_tool_cause_survives_failed_exhaustion_notice() {
    assert_stall_notice_cause(Mode::ToolStall);
}

#[test]
fn model_cancellation_survives_failed_final_accounting_presentation() {
    let mut record = Record::default();
    let (result, _, _) = run(Mode::CancelWithAccountingFailure, &mut record, None);
    assert!(result.is_err());
    let summary = record.summaries.last().unwrap();
    assert_eq!(
        summary.stop_reason,
        Some(sav::runtime::StopReason::Cancelled),
        "{summary:?}"
    );
    assert!(summary.cancelled);
    assert!(!summary.budget_exhausted);
}

#[test]
fn original_execution_failure_survives_failed_terminal_presentation() {
    let mut executor = Executor::new(Some(0));
    executor.failure = Some(sav::runtime::ToolFailureCategory::Unobserved);
    let (result, _, _) = run_custom(
        Mode::TerminalFailureWithTools,
        &mut Record::default(),
        RunLimits::default(),
        &executor,
        None,
    );
    let error = result.unwrap_err().describe();
    assert!(
        error.contains("executor failed after possible effects"),
        "{error}"
    );
    assert!(error.contains("event connection closed"), "{error}");
}
