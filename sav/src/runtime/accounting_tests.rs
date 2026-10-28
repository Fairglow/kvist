use super::*;
use crate::{
    FinishReason, LocalModelProvider, ModelMessage, ModelUsage, ReasoningEffort, ToolChoice,
    ToolDefinition, ToolIntent,
};
use std::sync::{Mutex, atomic::AtomicUsize};

fn session(tools: Vec<ToolDefinition>) -> AgentSession {
    let mut session = AgentSession::new(
        ModelIdentity {
            id: "selected".into(),
            model: "native-model".into(),
        },
        ReasoningEffort::Low,
        tools,
        "host instructions".into(),
    );
    session.push_user("original task");
    session
}

fn tool() -> ToolDefinition {
    ToolDefinition {
        name: "observe".into(),
        description: "Observe test data".into(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    }
}

fn answer() -> ModelTurn {
    ModelTurn {
        text: "done".into(),
        reasoning: None,
        tool_intents: vec![],
        finish_reason: FinishReason::Stop,
        provider: LocalModelProvider::LlamaServer,
        model: "native-model".into(),
        response_id: None,
        provider_request_id: None,
        usage: None,
    }
}

#[test]
fn native_request_uses_none_without_tools_and_auto_with_tools() {
    for tools in [vec![], vec![tool()]] {
        let expected = ModelRequest {
            model: "native-model".into(),
            messages: vec![
                ModelMessage::System("host instructions".into()),
                ModelMessage::User("original task".into()),
            ],
            tool_choice: if tools.is_empty() {
                ToolChoice::None
            } else {
                ToolChoice::Auto
            },
            tools: tools.clone(),
            reasoning_effort: Some(ReasoningEffort::Low),
            output_schema: None,
            max_output_tokens: None,
        };
        assert_eq!(session(tools).next_request(), Some(expected));
    }
}

#[test]
fn no_tool_native_request_passes_direct_transport_validation() {
    let request = session(vec![]).next_request().unwrap();
    crate::direct_transport::validate_request(&request).unwrap();
}

#[derive(Default)]
struct Sink {
    events: Mutex<Vec<Event>>,
    text_delay: Duration,
}

impl EventSink for Sink {
    fn send(&self, event: Event) -> Result<()> {
        if matches!(event, Event::Text(_)) {
            thread::sleep(self.text_delay);
        }
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

#[test]
fn decode_clock_needs_two_observations_and_excludes_callback_time() {
    let start = Instant::now();
    let mut clock = DecodeClock::default();
    assert_eq!(clock.seconds(), 0.0);
    clock.observe(start);
    assert_eq!(token_rate(10, clock.seconds()), 0.0);
    clock.exclude_handling(Duration::from_secs(2));
    clock.observe(start + Duration::from_secs(5));
    assert_eq!(clock.seconds(), 3.0);
    assert_eq!(token_rate(12, clock.seconds()), 4.0);
}

#[test]
fn live_progress_keeps_input_output_and_total_cumulative() {
    let session = session(vec![]);
    let context = ContextManager::new(8192, 4);
    let sink = Sink::default();
    let start = Instant::now();
    emit_live_progress(
        &sink,
        &LiveProgress {
            session: &session,
            context: &context,
            tool_defs: 0,
            started_at: start,
            cumulative_input: 100,
            cumulative_total: 120,
            cumulative_output: 20,
            decode_seconds: 2.0,
        },
        8,
        1.0,
        0.0,
        start + Duration::from_secs(5),
    )
    .unwrap();
    let events = sink.events.lock().unwrap();
    let Event::Progress {
        input_tokens,
        output_tokens,
        total_tokens,
        generation_tokens_per_sec,
        average_tokens_per_sec,
        elapsed_secs,
        ..
    } = events[0]
    else {
        panic!("expected progress")
    };
    assert_eq!((input_tokens, output_tokens, total_tokens), (100, 22, 122));
    assert_eq!(generation_tokens_per_sec, 2.0);
    assert!((average_tokens_per_sec - 22.0 / 3.0).abs() < 1e-9);
    assert_eq!(elapsed_secs, 5.0);
}

struct TimedProvider {
    prefill: Duration,
    decode: Vec<Duration>,
    attempts: AtomicUsize,
    observed_decode: Mutex<Duration>,
}

impl ModelTransport for TimedProvider {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> crate::Result<ModelTurn> {
        unreachable!()
    }

    fn stream(
        &self,
        _: &ModelRequest,
        _: &CancellationToken,
        emit: &mut dyn FnMut(ModelStreamEvent) -> crate::Result<()>,
    ) -> crate::Result<ModelTurn> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        thread::sleep(self.prefill);
        emit(ModelStreamEvent::TextDelta("abcd".into()))?;
        let start = Instant::now();
        thread::sleep(self.decode[attempt]);
        *self.observed_decode.lock().unwrap() += start.elapsed();
        emit(ModelStreamEvent::TextDelta("efgh".into()))?;
        if attempt + 1 < self.decode.len() {
            Err(crate::Error::ModelTransportTimedOut)
        } else {
            Ok(answer())
        }
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}

fn measured_decode(provider: &TimedProvider, sink: &Sink, backoff: Duration) -> f64 {
    let runtime = NativeRuntime::with_retry(
        5,
        RetryPolicy::new(provider.decode.len() as u32, backoff, backoff),
    );
    let session = session(vec![]);
    let context = ContextManager::new(8192, 4);
    let cancellation = CancellationToken::new();
    let budget = PromptBudget::start(Some(Duration::from_secs(10)), &cancellation).unwrap();
    let mut tokens = Some(100_000);
    runtime
        .drive_turn(
            &session.next_request().unwrap(),
            provider,
            sink,
            &cancellation,
            &LiveProgress {
                session: &session,
                context: &context,
                tool_defs: 0,
                started_at: Instant::now(),
                cumulative_input: 0,
                cumulative_total: 0,
                cumulative_output: 0,
                decode_seconds: 0.0,
            },
            &mut 0.0,
            &budget,
            &mut tokens,
            &mut None,
            &ControlMailbox::default(),
            1,
            &|intent| intent.name.clone(),
        )
        .unwrap()
        .1
}

#[test]
fn generation_timing_excludes_first_token_wait_and_presentation() {
    let provider = TimedProvider {
        prefill: Duration::from_millis(150),
        decode: vec![Duration::from_millis(20)],
        attempts: AtomicUsize::new(0),
        observed_decode: Mutex::new(Duration::ZERO),
    };
    let sink = Sink {
        text_delay: Duration::from_millis(60),
        ..Sink::default()
    };
    let seconds = measured_decode(&provider, &sink, Duration::ZERO);
    let observed = provider.observed_decode.lock().unwrap().as_secs_f64();
    assert!(
        (seconds - observed).abs() < 0.025,
        "decode {seconds}, observed {observed}: prefill and sink time must be excluded"
    );
}

#[test]
fn generation_timing_accumulates_retry_decode_without_backoff() {
    let provider = TimedProvider {
        prefill: Duration::ZERO,
        decode: vec![Duration::from_millis(130), Duration::from_millis(20)],
        attempts: AtomicUsize::new(0),
        observed_decode: Mutex::new(Duration::ZERO),
    };
    let seconds = measured_decode(&provider, &Sink::default(), Duration::from_millis(120));
    let observed = provider.observed_decode.lock().unwrap().as_secs_f64();
    assert!(
        (seconds - observed).abs() < 0.025,
        "decode {seconds}, observed {observed}: every attempt counts, backoff does not"
    );
}

struct Executor;
impl ToolExecutor for Executor {
    fn execute(&self, _: &ToolIntent, _: &CancellationToken) -> Result<ToolOutcome> {
        Ok(ToolOutcome::rejected_with("test observation unavailable"))
    }
}

struct TwoTurns(AtomicUsize);
impl ModelTransport for TwoTurns {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> crate::Result<ModelTurn> {
        unreachable!()
    }

    fn stream(
        &self,
        _: &ModelRequest,
        _: &CancellationToken,
        emit: &mut dyn FnMut(ModelStreamEvent) -> crate::Result<()>,
    ) -> crate::Result<ModelTurn> {
        let first = self.0.fetch_add(1, Ordering::SeqCst) == 0;
        emit(ModelStreamEvent::TextDelta("abcd".into()))?;
        thread::sleep(Duration::from_millis(270));
        emit(ModelStreamEvent::TextDelta("efgh".into()))?;
        let mut turn = answer();
        turn.usage = Some(if first {
            ModelUsage {
                input_tokens: 100,
                output_tokens: 20,
                total_tokens: 125,
            }
        } else {
            ModelUsage {
                input_tokens: 10,
                output_tokens: 100,
                total_tokens: 112,
            }
        });
        if first {
            turn.finish_reason = FinishReason::ToolCalls;
            turn.tool_intents.push(ToolIntent {
                id: "observation-1".into(),
                provider_id: None,
                name: "observe".into(),
                arguments: serde_json::json!({}),
            });
        }
        Ok(turn)
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}

#[test]
fn native_live_totals_preserve_provider_usage_and_post_turn_rate_convention() {
    let sink = Sink::default();
    let result = NativeRuntime::default()
        .run(
            &mut session(vec![tool()]),
            &TwoTurns(AtomicUsize::new(0)),
            &Executor,
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(8192, 4),
            None,
        )
        .unwrap();
    assert!(result.success());
    let events = sink.events.lock().unwrap();
    let live = events
        .iter()
        .rev()
        .find(|event| {
            matches!(
                event,
                Event::Progress {
                    token_accounting: TokenAccounting::Estimated,
                    ..
                }
            )
        })
        .unwrap();
    assert!(matches!(
        live,
        Event::Progress {
            input_tokens: 100,
            output_tokens: 22,
            total_tokens: 127,
            ..
        }
    ));
    let last = events
        .iter()
        .rev()
        .find(|event| matches!(event, Event::Progress { .. }))
        .unwrap();
    let Event::Progress {
        input_tokens,
        output_tokens,
        total_tokens,
        generation_tokens_per_sec,
        average_tokens_per_sec,
        ..
    } = last
    else {
        unreachable!()
    };
    assert_eq!(
        (*input_tokens, *output_tokens, *total_tokens),
        (110, 120, 237)
    );
    assert_eq!(generation_tokens_per_sec, average_tokens_per_sec);
}

#[test]
fn progress_elapsed_uses_prompt_start_including_initial_recording() {
    let runtime = NativeRuntime::default();
    let cancellation = CancellationToken::new();
    let budget = PromptBudget::start(Some(Duration::from_secs(10)), &cancellation).unwrap();
    thread::sleep(Duration::from_millis(100));
    let sink = Sink::default();
    let controls = ControlMailbox::default();
    controls.begin_run().unwrap();
    runtime
        .run_inner(
            &mut session(vec![]),
            &TimedProvider {
                prefill: Duration::ZERO,
                decode: vec![Duration::ZERO],
                attempts: AtomicUsize::new(0),
                observed_decode: Mutex::new(Duration::ZERO),
            },
            &Executor,
            &sink,
            &cancellation,
            &mut ContextManager::new(8192, 4),
            &mut None,
            &budget,
            &mut RunSummary::default(),
            &controls,
        )
        .unwrap();
    let events = sink.events.lock().unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Progress { elapsed_secs, .. } if *elapsed_secs >= 0.1
    )));
}
