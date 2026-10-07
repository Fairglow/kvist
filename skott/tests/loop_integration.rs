// End-to-end tests for the transport-agnostic agent loop.
//
// These drive `AgentRunner::run` directly with a scripted `ModelTransport` and
// a recording `ToolExecutor`, so they exercise the real loop policy - turn
// folding, tool execution, result feedback, compaction, cancellation, and
// progress accounting - without spawning a sandbox or talking to a model.
//
// The durable record is asserted to keep every reasoning trace even after the
// model context is compacted, which is the auditing guarantee the component
// exists to preserve.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use sav::{
    CancellationToken, FinishReason, LocalModelProvider, ModelMessage, ModelRequest,
    ModelStreamEvent, ModelTransport, ModelTurn, ModelUsage, ReasoningEffort, ToolIntent,
};
use serde_json::json;

use skott::{
    AgentRunner, AgentSession, ContextManager, DEFAULT_CONTEXT_TOKENS, Error, Event, EventSink,
    MAX_TURNS, Model, ModelProvider, Recorder, RetryPolicy, RunSummary, ToolExecutor, ToolOutcome,
    ToolPolicy, ToolRegistry,
};

/// A transport that yields a scripted sequence of turns, streaming each one's
/// reasoning, text, and tool intents before returning the assembled turn.
struct ScriptedTransport {
    turns: Arc<Mutex<Vec<ModelTurn>>>,
}

impl ScriptedTransport {
    fn new(turns: Vec<ModelTurn>) -> Self {
        ScriptedTransport {
            turns: Arc::new(Mutex::new(turns)),
        }
    }
}

impl ModelTransport for ScriptedTransport {
    fn complete(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
    ) -> sav::Result<ModelTurn> {
        Err(sav::Error::ModelTransportCancelled)
    }

    fn stream(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        let turn = {
            let mut turns = self.turns.lock().unwrap();
            if turns.is_empty() {
                answer_turn("(no more turns)")
            } else {
                turns.remove(0)
            }
        };
        if let Some(reasoning) = &turn.reasoning {
            on_event(ModelStreamEvent::ReasoningDelta(reasoning.clone()))?;
        }
        if !turn.text.is_empty() {
            on_event(ModelStreamEvent::TextDelta(turn.text.clone()))?;
        }
        for intent in &turn.tool_intents {
            on_event(ModelStreamEvent::ToolIntent(intent.clone()))?;
        }
        Ok(turn)
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(30)
    }
}

/// A tool-proposing turn: reasoning + text + a single tool intent.
fn tool_turn(text: &str, tool: &str, arguments: serde_json::Value) -> ModelTurn {
    ModelTurn {
        text: text.to_owned(),
        reasoning: Some(format!("reasoning while {text}")),
        tool_intents: vec![ToolIntent {
            id: "call-1".to_owned(),
            provider_id: None,
            name: tool.to_owned(),
            arguments,
        }],
        finish_reason: FinishReason::ToolCalls,
        provider: LocalModelProvider::LlamaServer,
        model: "test-model".to_owned(),
        response_id: None,
        provider_request_id: None,
        usage: Some(ModelUsage {
            input_tokens: 20,
            output_tokens: 10,
            total_tokens: 30,
        }),
    }
}

/// A terminal turn with no tool intents; its text is the session answer.
fn answer_turn(text: &str) -> ModelTurn {
    ModelTurn {
        text: text.to_owned(),
        reasoning: Some(format!("reasoning to conclude: {text}")),
        tool_intents: vec![],
        finish_reason: FinishReason::Stop,
        provider: LocalModelProvider::LlamaServer,
        model: "test-model".to_owned(),
        response_id: None,
        provider_request_id: None,
        usage: Some(ModelUsage {
            input_tokens: 12,
            output_tokens: 6,
            total_tokens: 18,
        }),
    }
}

#[test]
fn invalid_injected_turn_limits_fail_before_model_io() {
    for limit in [0, MAX_TURNS + 1, u32::MAX] {
        let transport = ScriptedTransport::new(vec![answer_turn("not sent")]);
        let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
        let mut session = make_session();
        session.push_user("go");
        let result = AgentRunner::with_retry(limit, fast_retry()).run(
            &mut session,
            &transport,
            &executor,
            &Collector::default(),
            &CancellationToken::new(),
            &mut ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
            None,
        );
        assert!(matches!(result, Err(Error::Config { .. })));
        assert_eq!(transport.turns.lock().unwrap().len(), 1);
    }
}

fn success_outcome(stdout: &str) -> ToolOutcome {
    ToolOutcome {
        exited: true,
        status: Some(0),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
        timed_out: false,
        output_limit_exceeded: false,
        cancelled: false,
    }
}

/// An executor that enforces the shell denylist like the sandbox does, actually
/// performs `write_file` into the working directory, and records every approved
/// call for assertions.
struct RecordingExecutor {
    policy: ToolPolicy,
    workdir: PathBuf,
    executed: Arc<Mutex<Vec<(String, String)>>>,
    cancel_after: Option<CancellationToken>,
}

impl RecordingExecutor {
    fn new(policy: ToolPolicy, workdir: PathBuf) -> Self {
        RecordingExecutor {
            policy,
            workdir,
            executed: Arc::new(Mutex::new(Vec::new())),
            cancel_after: None,
        }
    }

    fn executed(&self) -> Vec<(String, String)> {
        self.executed.lock().unwrap().clone()
    }
}

impl ToolExecutor for RecordingExecutor {
    fn execute(
        &self,
        intent: &ToolIntent,
        _cancellation: &CancellationToken,
    ) -> skott::Result<ToolOutcome> {
        if intent.name == "shell" {
            let command = intent
                .arguments
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if !self.policy.shell_permitted(command) {
                return Err(Error::ToolPolicy {
                    tool: "shell".to_owned(),
                    reason: "command matches the forbidden-command policy".to_owned(),
                });
            }
        }
        if intent.name == "write_file" {
            let path = intent
                .arguments
                .get("path")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let content = intent
                .arguments
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let target = self.workdir.join(path.trim_start_matches('/'));
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&target, content).expect("staged write stays inside the workdir");
            self.record(intent.name.clone(), format!("wrote {path}"));
            return Ok(success_outcome(&format!("wrote {}", path)));
        }
        self.record(intent.name.clone(), "ok".to_owned());
        Ok(success_outcome("ok"))
    }
}

impl RecordingExecutor {
    fn record(&self, tool: String, summary: String) {
        self.executed.lock().unwrap().push((tool, summary));
        if let Some(token) = &self.cancel_after {
            token.cancel();
        }
    }
}

/// Collects every event the loop emits so tests can assert on the stream.
#[derive(Default)]
struct Collector {
    events: Arc<Mutex<Vec<Event>>>,
}

impl EventSink for Collector {
    fn send(&self, event: Event) -> skott::Result<()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

impl Collector {
    fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap().clone()
    }
}

/// In-memory recorder that proves the durable record keeps every reasoning
/// trace, even after the model context is compacted.
#[derive(Default)]
struct FakeRecorder {
    turns: AtomicUsize,
    reasoning: Arc<Mutex<Vec<String>>>,
    tool_results: Arc<Mutex<Vec<String>>>,
    session_started: AtomicBool,
    session_finished: AtomicBool,
    terminal_calls: AtomicUsize,
    fail_dispatch: bool,
    fail_notice: bool,
}

impl Recorder for FakeRecorder {
    fn notice(&mut self, _message: &str) -> skott::Result<()> {
        if self.fail_notice {
            return Err(Error::Recording {
                reason: "injected notice failure".into(),
            });
        }
        Ok(())
    }
    fn session_start(&mut self) -> skott::Result<()> {
        self.session_started.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn request(&mut self, _request: &ModelRequest, _attempt: u32) -> skott::Result<()> {
        Ok(())
    }

    fn turn_start(&mut self, turn: &ModelTurn) -> skott::Result<usize> {
        let index = self.turns.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(reasoning) = &turn.reasoning {
            self.reasoning.lock().unwrap().push(reasoning.clone());
        }
        Ok(index)
    }

    fn turn_finish(
        &mut self,
        _turn: usize,
        _usage: &Option<ModelUsage>,
        _finish_reason: &str,
    ) -> skott::Result<()> {
        Ok(())
    }

    fn tool_dispatch(&mut self, _turn: usize, _intent: &ToolIntent) -> skott::Result<()> {
        if self.fail_dispatch {
            return Err(Error::Recording {
                reason: "injected pre-effect write failure".into(),
            });
        }
        Ok(())
    }

    fn tool_result(
        &mut self,
        turn: usize,
        _call_id: &str,
        tool: &str,
        _args: &serde_json::Value,
        outcome: &ToolOutcome,
    ) -> skott::Result<()> {
        self.tool_results
            .lock()
            .unwrap()
            .push(format!("turn {turn} {tool} failed={}", outcome.failed()));
        Ok(())
    }

    fn session_finish(&mut self, summary: &RunSummary) -> skott::Result<()> {
        self.session_finished
            .store(summary.success(), Ordering::SeqCst);
        self.terminal_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl FakeRecorder {
    fn reasoning(&self) -> Vec<String> {
        self.reasoning.lock().unwrap().clone()
    }
}

fn make_session() -> AgentSession {
    make_session_with_tools(ToolRegistry::new(ToolPolicy::minimum()).tool_definitions())
}

fn make_session_with_tools(tool_defs: Vec<sav::ToolDefinition>) -> AgentSession {
    let model = Model {
        context_limit: None,
        response_reserve: None,
        id: "test".to_owned(),
        provider: ModelProvider::LlamaServer,
        base_url: "http://127.0.0.1:9931".to_owned(),
        model: "test-model".to_owned(),
        is_default: false,
        deadline_secs: 30,
        max_attempts: 3,
        retry_base_delay_secs: 2,
        retry_max_delay_secs: 30,
        cadence_timeout_secs: 30,
    };
    AgentSession::new(
        model,
        ReasoningEffort::Medium,
        tool_defs,
        "system".to_owned(),
    )
}

fn run_once(
    session: &mut AgentSession,
    transport: &ScriptedTransport,
    executor: &RecordingExecutor,
    sink: &Collector,
    cancellation: &CancellationToken,
    context: &mut ContextManager,
    recorder: &mut FakeRecorder,
) -> RunSummary {
    let runner = AgentRunner::default();
    runner
        .run(
            session,
            transport,
            executor,
            sink,
            cancellation,
            context,
            Some(&mut *recorder),
        )
        .expect("loop completes without error")
}

fn run_collected(
    session: &mut AgentSession,
    transport: &ScriptedTransport,
    executor: &RecordingExecutor,
    cancellation: &CancellationToken,
    context: &mut ContextManager,
    recorder: &mut FakeRecorder,
) -> (RunSummary, Collector) {
    let sink = Collector::default();
    let summary = run_once(
        session,
        transport,
        executor,
        &sink,
        cancellation,
        context,
        recorder,
    );
    (summary, sink)
}

#[test]
fn loop_without_tool_intents_delivers_the_answer() {
    let transport = ScriptedTransport::new(vec![answer_turn("Here is the answer")]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("what is two plus two?");
    let (summary, sink) = run_collected(
        &mut session,
        &transport,
        &executor,
        &cancellation,
        &mut context,
        &mut recorder,
    );

    assert_eq!(summary.turns, 1);
    assert_eq!(summary.tools_executed, 0);
    assert_eq!(summary.answer.as_deref(), Some("Here is the answer"));
    assert!(recorder.session_started.load(Ordering::SeqCst));
    assert!(recorder.session_finished.load(Ordering::SeqCst));
    let events = sink.events();
    assert!(events.iter().any(|e| matches!(e, Event::TurnStart { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::Reasoning(_))));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Finished { message } if message == "Here is the answer"
    )));
    assert!(executor.executed().is_empty());
}

#[test]
fn tool_call_is_executed_and_result_is_fed_back() {
    let transport = ScriptedTransport::new(vec![
        tool_turn(
            "running the command",
            "shell",
            json!({ "command": "echo hi" }),
        ),
        answer_turn("done"),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("run it");
    let (summary, sink) = run_collected(
        &mut session,
        &transport,
        &executor,
        &cancellation,
        &mut context,
        &mut recorder,
    );

    assert_eq!(summary.turns, 2);
    assert_eq!(summary.tools_executed, 1);
    assert_eq!(summary.answer.as_deref(), Some("done"));
    assert_eq!(executor.executed().len(), 1);
    let events = sink.events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::ToolCall { name, .. } if name == "shell"))
    );
    assert!(events.iter().any(|e| matches!(
        e,
        Event::ToolResult { name, failed, .. } if name == "shell" && !failed
    )));
    assert_eq!(recorder.turns.load(Ordering::SeqCst), 2);
    assert_eq!(recorder.reasoning().len(), 2);
}

#[test]
fn a_denied_shell_command_is_reported_but_not_fatal() {
    // A denylisted destructive command; the executor refuses it before any
    // sandbox request, and the loop must keep going.
    let transport = ScriptedTransport::new(vec![
        tool_turn(
            "destroying everything",
            "shell",
            json!({ "command": "rm -rf /" }),
        ),
        answer_turn("refused and recovered"),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("please go");
    let (summary, sink) = run_collected(
        &mut session,
        &transport,
        &executor,
        &cancellation,
        &mut context,
        &mut recorder,
    );

    assert_eq!(summary.turns, 2);
    assert_eq!(summary.tools_executed, 0);
    assert_eq!(summary.answer.as_deref(), Some("refused and recovered"));
    assert!(executor.executed().is_empty());
    let events = sink.events();
    assert!(events.iter().any(|e| matches!(
        e,
        Event::ToolResult { name, failed, .. } if name == "shell" && *failed
    )));
    assert!(events.iter().any(|e| matches!(e, Event::Note(_))));
    assert!(!events.iter().any(|e| matches!(e, Event::Failed(_))));
}

#[test]
fn an_approved_write_reaches_the_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    let transport = ScriptedTransport::new(vec![
        tool_turn(
            "writing the file",
            "write_file",
            json!({ "path": "output.txt", "content": "hello sandbox" }),
        ),
        answer_turn("wrote the file"),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), dir.path().to_path_buf());
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("create output.txt");
    let (summary, _sink) = run_collected(
        &mut session,
        &transport,
        &executor,
        &cancellation,
        &mut context,
        &mut recorder,
    );

    assert_eq!(summary.tools_executed, 1);
    let written = std::fs::read_to_string(dir.path().join("output.txt")).unwrap();
    assert_eq!(written, "hello sandbox");
}

#[test]
fn context_compacts_across_prompts_but_the_record_keeps_everything() {
    // A small window forces compaction once several prompts accumulate. Each
    // prompt is its own run, mirroring how the worker threads multiple prompts
    // through the same session and context manager.
    let transport = ScriptedTransport::new(vec![
        tool_turn(
            "working on the first task and reporting back now",
            "shell",
            json!({ "command": "echo first task output" }),
        ),
        answer_turn(&"finished first ".repeat(100)),
        tool_turn(
            "working on the second task and reporting back now",
            "shell",
            json!({ "command": "echo second task output" }),
        ),
        answer_turn(&"finished second ".repeat(100)),
        tool_turn(
            "working on the third task and reporting back now",
            "shell",
            json!({ "command": "echo third task output" }),
        ),
        answer_turn("finished third"),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    // Includes full instructions/schema and the enforced 1024-token reserve.
    let mut context = ContextManager::new(2000, 1);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session_with_tools(vec![sav::ToolDefinition {
        name: "shell".into(),
        description: "Run a sandboxed command".into(),
        parameters: json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]}),
    }]);
    let mut last_answer = None;
    for prompt in ["task one", "task two", "task three"] {
        session.push_user(prompt);
        let (summary, _sink) = run_collected(
            &mut session,
            &transport,
            &executor,
            &cancellation,
            &mut context,
            &mut recorder,
        );
        cancellation.reset();
        last_answer = summary.answer.clone();
    }

    // The live context was rolled into a rolling summary...
    assert!(
        !context.summary().is_empty(),
        "expected a rolling summary after compaction"
    );
    // ...but every turn's reasoning is still in the durable record.
    assert_eq!(recorder.turns.load(Ordering::SeqCst), 6);
    assert_eq!(recorder.reasoning().len(), 6);
    assert_eq!(last_answer.as_deref(), Some("finished third"));
}

/// Whether a request carries a given user message verbatim.
fn carries_user_message(messages: &[ModelMessage], text: &str) -> bool {
    messages
        .iter()
        .any(|message| matches!(message, ModelMessage::User(found) if found == text))
}

#[test]
fn a_follow_up_prompt_carries_the_earlier_prompt_in_context() {
    // Within one session the model conversation accumulates, so a follow-up
    // ("continue") is sent with the whole prior conversation. This is why the
    // agent can pick up an earlier task instead of starting blind, and it is the
    // guarantee the interactive UI relies on when the user asks it to continue.
    let transport = CapturingTransport::new(vec![answer_turn("first"), answer_turn("second")]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("first task text");
    let summary1 = AgentRunner::default()
        .run(
            &mut session,
            &transport,
            &executor,
            &Collector::default(),
            &cancellation,
            &mut context,
            Some(&mut recorder),
        )
        .expect("first prompt completes");
    assert_eq!(summary1.answer.as_deref(), Some("first"));

    session.push_user("second task text");
    let summary2 = AgentRunner::default()
        .run(
            &mut session,
            &transport,
            &executor,
            &Collector::default(),
            &cancellation,
            &mut context,
            Some(&mut recorder),
        )
        .expect("follow-up completes");
    assert_eq!(summary2.answer.as_deref(), Some("second"));

    let requests = transport.requests();
    assert_eq!(requests.len(), 2, "one request per prompt");
    assert!(
        carries_user_message(&requests[0].messages, "first task text"),
        "the first prompt is in its own request"
    );
    assert!(
        carries_user_message(&requests[1].messages, "first task text"),
        "the follow-up did not carry the earlier prompt's text"
    );
    assert!(
        carries_user_message(&requests[1].messages, "second task text"),
        "the follow-up prompt is missing"
    );
}

#[test]
fn progress_events_report_speed_utilization_and_compaction_progress() {
    let transport = ScriptedTransport::new(vec![answer_turn("all done")]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("do a thing");
    let (_summary, sink) = run_collected(
        &mut session,
        &transport,
        &executor,
        &cancellation,
        &mut context,
        &mut recorder,
    );

    let progress = sink
        .events()
        .iter()
        .filter(|e| matches!(e, Event::Progress { .. }))
        .count();
    assert!(
        progress >= 1,
        "expected at least one progress event, got {progress}"
    );
    for event in sink.events() {
        if let Event::Progress {
            context_limit,
            generation_tokens_per_sec,
            average_tokens_per_sec,
            context_utilization,
            compaction_progress,
            ..
        } = event
        {
            assert!(context_limit > 0);
            assert!(generation_tokens_per_sec.is_finite());
            assert!(generation_tokens_per_sec >= 0.0);
            assert!(average_tokens_per_sec >= 0.0);
            assert!(context_utilization >= 0.0);
            assert!((0.0..=1.0).contains(&compaction_progress));
        }
    }
}

#[test]
fn progress_is_emitted_after_each_turn_with_cumulative_totals() {
    // A three-turn session (two tool turns + one answer) must report progress
    // after every turn — plus the final report after the loop — carrying the
    // cumulative input/output token totals of the scripted provider usage, so
    // the live stats bar tracks the session while it runs.
    let transport = ScriptedTransport::new(vec![
        tool_turn(
            "looking at the first part",
            "shell",
            json!({ "command": "echo one" }),
        ),
        tool_turn(
            "looking at the second part",
            "shell",
            json!({ "command": "echo two" }),
        ),
        answer_turn("all done"),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("do the thing");
    let (summary, sink) = run_collected(
        &mut session,
        &transport,
        &executor,
        &cancellation,
        &mut context,
        &mut recorder,
    );

    assert_eq!(summary.turns, 3);
    let progress: Vec<Event> = sink
        .events()
        .into_iter()
        .filter(|e| matches!(e, Event::Progress { .. }))
        .collect();
    // Exactly one progress report per turn; the final turn's report is the
    // session's last accounting, emitted before the loop stops.
    assert_eq!(progress.len(), 3, "one progress report per turn");
    let Event::Progress {
        input_tokens,
        output_tokens,
        total_tokens,
        ..
    } = progress.last().expect("a progress report exists")
    else {
        panic!("last event is a Progress");
    };
    // tool_turn usage is 20 in / 10 out / 30 total (twice); the answer turn is
    // 12 in / 6 out / 18 total.
    assert_eq!(*input_tokens, 52);
    assert_eq!(*output_tokens, 26);
    assert_eq!(*total_tokens, 78);
}

#[test]
fn cancellation_before_a_tool_is_executed_stops_the_loop() {
    let transport = ScriptedTransport::new(vec![
        tool_turn("about to work", "shell", json!({ "command": "echo hi" })),
        answer_turn("would have been the answer"),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    // Cancel before running: the loop must gate on the token before executing.
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let mut session = make_session();
    session.push_user("go");
    let (summary, sink) = run_collected(
        &mut session,
        &transport,
        &executor,
        &cancellation,
        &mut context,
        &mut recorder,
    );

    assert!(summary.cancelled);
    assert_eq!(summary.tools_executed, 0);
    assert!(executor.executed().is_empty());
    let events = sink.events();
    assert_eq!(
        summary.turns, 0,
        "pre-cancelled prompts never reach the provider"
    );
    assert!(!events.iter().any(|e| matches!(e, Event::TurnStart { .. })));
    assert!(!events.iter().any(|e| matches!(e, Event::ToolResult { .. })));
    assert_eq!(recorder.terminal_calls.load(Ordering::SeqCst), 1);
}

/// A transport that fails the first `failures` attempts with a transient error,
/// then yields a single successful turn. Exercises the loop's retry/back-off
/// without any real network or timing cost.
struct RetryingTransport {
    failures: usize,
    attempts: Arc<Mutex<usize>>,
    turn: ModelTurn,
}

impl RetryingTransport {
    fn new(failures: usize, turn: ModelTurn) -> Self {
        RetryingTransport {
            failures,
            attempts: Arc::new(Mutex::new(0)),
            turn,
        }
    }
}

impl ModelTransport for RetryingTransport {
    fn complete(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
    ) -> sav::Result<ModelTurn> {
        Err(sav::Error::ModelTransportCancelled)
    }

    fn stream(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        let mut attempts = self.attempts.lock().unwrap();
        *attempts += 1;
        let attempt = *attempts;
        drop(attempts);

        if attempt <= self.failures {
            // A temporal, recoverable failure: the provider timed out mid-turn.
            return Err(sav::Error::ModelTransportTimedOut);
        }

        if let Some(reasoning) = &self.turn.reasoning {
            on_event(ModelStreamEvent::ReasoningDelta(reasoning.clone()))?;
        }
        if !self.turn.text.is_empty() {
            on_event(ModelStreamEvent::TextDelta(self.turn.text.clone()))?;
        }
        Ok(self.turn.clone())
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(30)
    }
}

fn fast_retry() -> RetryPolicy {
    // A tiny budget and sub-millisecond back-off so the loop recovers quickly
    // in tests while still exercising the real back-off code path.
    RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(5))
}

#[test]
fn llama_wire_stop_or_missing_finish_with_tools_never_dispatches() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Instant;

    for finish in [json!("stop"), serde_json::Value::Null] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let body = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({
                "model": "test-model",
                "choices": [{
                    "index": 0,
                    "delta": {"tool_calls": [{
                        "index": 0, "id": "reject-call", "type": "function",
                        "function": {"name": "shell", "arguments": "{\"command\":\"echo unsafe\"}"}
                    }]},
                    "finish_reason": finish
                }]
            })
        );
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "provider was not contacted");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let header_end = loop {
                let mut bytes = [0; 4096];
                let count = socket.read(&mut bytes).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&bytes[..count]);
                assert!(request.len() <= 65536);
                if let Some(position) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                    break position + 4;
                }
            };
            let length = std::str::from_utf8(&request[..header_end])
                .unwrap()
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            assert!(header_end + length <= 65536);
            while request.len() < header_end + length {
                let mut bytes = [0; 4096];
                let count = socket.read(&mut bytes).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&bytes[..count]);
            }
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let model = Model {
            id: "wire-test".into(),
            provider: ModelProvider::LlamaServer,
            base_url: endpoint,
            model: "test-model".into(),
            is_default: false,
            context_limit: None,
            response_reserve: None,
            deadline_secs: 5,
            max_attempts: 1,
            retry_base_delay_secs: 1,
            retry_max_delay_secs: 1,
            cadence_timeout_secs: 1,
        };
        let transport = model.transport().unwrap();
        let mut session = AgentSession::new(
            model,
            ReasoningEffort::None,
            ToolRegistry::new(ToolPolicy::minimum()).tool_definitions(),
            "system".into(),
        );
        session.push_user("go");
        let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
        let sink = Collector::default();
        let result = AgentRunner::with_retry(1, fast_retry()).run(
            &mut session,
            &transport,
            &executor,
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
            None,
        );
        server.join().unwrap();
        let summary = result.unwrap();
        assert_eq!(summary.tools_executed, 0);
        assert!(!summary.success());
        assert!(summary.answer.is_none());
        assert!(executor.executed.lock().unwrap().is_empty());
        assert!(sink.events().iter().any(|e| matches!(e, Event::Failed(_))));
    }
}

#[test]
fn hardening_truncated_or_filtered_turns_never_execute_tools_or_finish() {
    for reason in [
        FinishReason::Stop,
        FinishReason::Length,
        FinishReason::ContentFilter,
        FinishReason::Other("unknown".into()),
    ] {
        let mut proposal = tool_turn("incomplete", "shell", json!({"command": "echo unsafe"}));
        proposal.finish_reason = reason;
        let transport = ScriptedTransport::new(vec![proposal; 3]);
        let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
        let mut session = make_session();
        session.push_user("go");
        let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
        let mut recorder = FakeRecorder::default();
        let (summary, sink) = run_collected(
            &mut session,
            &transport,
            &executor,
            &CancellationToken::new(),
            &mut context,
            &mut recorder,
        );
        assert_eq!(
            summary.tools_executed, 0,
            "truncated proposals cannot cause effects"
        );
        assert!(summary.answer.is_none());
        assert!(sink.events().iter().any(|e| matches!(e, Event::Failed(_))));
    }
}

#[test]
fn hardening_length_without_tools_is_not_a_final_answer() {
    let mut value = answer_turn("partial answer");
    value.finish_reason = FinishReason::Length;
    let transport = ScriptedTransport::new(vec![value; 3]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("go");
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let (summary, _) = run_collected(
        &mut session,
        &transport,
        &executor,
        &CancellationToken::new(),
        &mut context,
        &mut recorder,
    );
    assert!(summary.answer.is_none(), "length termination is incomplete");
}

#[test]
fn hardening_duplicate_call_ids_reject_the_entire_turn() {
    let mut proposal = tool_turn("work", "shell", json!({"command": "echo first"}));
    proposal.tool_intents.push(proposal.tool_intents[0].clone());
    let transport = ScriptedTransport::new(vec![proposal, answer_turn("done")]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("go");
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let (summary, _) = run_collected(
        &mut session,
        &transport,
        &executor,
        &CancellationToken::new(),
        &mut context,
        &mut recorder,
    );
    assert_eq!(summary.tools_executed, 0);
}

#[test]
fn hardening_previous_answer_is_not_returned_after_a_failed_followup() {
    let transport = ScriptedTransport::new(vec![answer_turn("old answer")]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    session.push_user("first");
    let (first, _) = run_collected(
        &mut session,
        &transport,
        &executor,
        &CancellationToken::new(),
        &mut context,
        &mut recorder,
    );
    assert_eq!(first.answer.as_deref(), Some("old answer"));
    session.push_user("next");
    let failure = RetryingTransport::new(usize::MAX, answer_turn("never"));
    let second = AgentRunner::with_retry(1, fast_retry())
        .run(
            &mut session,
            &failure,
            &executor,
            &Collector::default(),
            &CancellationToken::new(),
            &mut context,
            Some(&mut recorder),
        )
        .unwrap();
    assert!(second.answer.is_none());
}

#[test]
fn hardening_tool_output_status_and_combined_preview_are_bounded() {
    let mut session = make_session();
    session.push_user("inspect");
    session.apply_assistant(tool_turn("read", "shell", json!({"command": "echo ok"})));
    let mut outcome = success_outcome(&"x".repeat(100_000));
    outcome.stderr = vec![b'y'; 100_000];
    outcome.status = Some(7);
    session.record_tool_result("call-1", "shell", &outcome);
    let request = session.next_request().unwrap();
    let Some(ModelMessage::ToolResult { content, .. }) = request.messages.last() else {
        panic!("result present");
    };
    assert!(
        content.len() <= 8_192,
        "combined output is one bounded preview"
    );
    assert!(content.contains("7"), "the model must see the exit status");
    assert!(
        content.contains("truncated"),
        "omitted output must be visible"
    );
}

#[test]
fn hardening_no_status_timeout_and_cancellation_are_failed_outcomes() {
    assert!(ToolOutcome::rejected_with("denied").failed());
    let mut outcome = success_outcome("");
    outcome.timed_out = true;
    assert!(outcome.failed());
    outcome.timed_out = false;
    outcome.cancelled = true;
    assert!(outcome.failed());
}

#[test]
fn cancelled_multicall_turn_is_valid_for_a_subsequent_prompt() {
    let cancellation = CancellationToken::new();
    let mut proposal = tool_turn("first", "shell", json!({"command":"echo first"}));
    let mut second = proposal.tool_intents[0].clone();
    second.id = "call-2".into();
    second.arguments = json!({"command":"echo second"});
    proposal.tool_intents.push(second);
    let transport = ScriptedTransport::new(vec![proposal, answer_turn("continued")]);
    let mut executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    executor.cancel_after = Some(cancellation.clone());
    let mut session = make_session();
    session.push_user("start");
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let (interrupted, _) = run_collected(
        &mut session,
        &transport,
        &executor,
        &cancellation,
        &mut context,
        &mut recorder,
    );
    assert!(interrupted.cancelled);
    assert_eq!(interrupted.tools_executed, 1);
    session.push_user("continue");
    let (continued, _) = run_collected(
        &mut session,
        &transport,
        &executor,
        &CancellationToken::new(),
        &mut context,
        &mut recorder,
    );
    assert!(continued.success(), "{continued:?}");
    assert_eq!(continued.answer.as_deref(), Some("continued"));
    assert_eq!(executor.executed().len(), 1);
}

#[test]
fn hardening_unchanged_repeated_actions_are_not_executed_again() {
    let proposal = tool_turn("repeat", "shell", json!({"command": "echo same"}));
    let transport =
        ScriptedTransport::new(vec![proposal.clone(), proposal, answer_turn("stopped")]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("work");
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let (summary, sink) = run_collected(
        &mut session,
        &transport,
        &executor,
        &CancellationToken::new(),
        &mut context,
        &mut recorder,
    );
    assert_eq!(summary.tools_executed, 1);
    assert_eq!(summary.answer.as_deref(), Some("stopped"));
    assert!(sink.events().iter().any(|e| matches!(e, Event::Note(_))));
}

#[test]
fn reliability_legitimate_native_rereads_do_not_trip_effect_breakers() {
    let proposal = tool_turn("refresh", "read_file", json!({"path":"/workspace/file"}));
    let transport = ScriptedTransport::new(vec![
        proposal.clone(),
        proposal.clone(),
        proposal.clone(),
        proposal.clone(),
        proposal,
        answer_turn("done"),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("refresh state");
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let (summary, _) = run_collected(
        &mut session,
        &transport,
        &executor,
        &CancellationToken::new(),
        &mut context,
        &mut recorder,
    );
    assert!(summary.success(), "{summary:?}");
    assert_eq!(summary.tools_executed, 5);
}

#[test]
fn reliability_length_is_regenerated_before_any_tool_effects() {
    let mut partial = tool_turn("partial", "shell", json!({"command":"echo unsafe-partial"}));
    partial.finish_reason = FinishReason::Length;
    let transport = CapturingTransport::new(vec![partial, answer_turn("complete")]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("work");
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let sink = Collector::default();
    let summary = AgentRunner::default()
        .run(
            &mut session,
            &transport,
            &executor,
            &sink,
            &CancellationToken::new(),
            &mut context,
            Some(&mut recorder),
        )
        .unwrap();
    assert!(summary.success(), "{summary:?}");
    assert!(executor.executed().is_empty());
    let requests = transport.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].max_output_tokens, Some(1024));
    assert_eq!(requests[1].max_output_tokens, Some(2048));
    assert_eq!(requests[0].messages, requests[1].messages);
    assert!(
        sink.events()
            .iter()
            .any(|e| matches!(e, Event::Note(text) if text.contains("length")))
    );
}

#[test]
fn reliability_binary_result_does_not_expand_into_an_oversized_json_prompt() {
    let mut session = make_session();
    session.push_user("inspect");
    session.record_tool_result(
        "binary",
        "shell",
        &ToolOutcome {
            stdout: vec![0; 6000],
            ..success_outcome("")
        },
    );
    let request = session.next_request().unwrap();
    let preview = match request.messages.last().unwrap() {
        ModelMessage::ToolResult { content, .. } => content,
        _ => panic!("tool result"),
    };
    assert!(preview.contains("binary"));
    assert!(serde_json::to_vec(preview).unwrap().len() < 2048);
    assert!(preview.contains("status=Some(0)"));
}

#[test]
fn reliability_escaped_text_preview_fits_the_encoded_budget() {
    let mut session = make_session();
    session.record_tool_result(
        "escaped",
        "shell",
        &success_outcome(&"\u{0001}".repeat(8192)),
    );
    let request = session.next_request().unwrap();
    let ModelMessage::ToolResult { content, .. } = request.messages.last().unwrap() else {
        panic!("tool result");
    };
    assert!(serde_json::to_vec(content).unwrap().len() <= 8192);
    assert!(content.contains("truncated"));
    assert!(content.contains("status=Some(0)"));
}

#[test]
fn reliability_native_page_metadata_survives_outer_model_json_escaping() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quoted.txt");
    let text = "\"\\\n".repeat(4000);
    std::fs::write(&path, &text).unwrap();
    let request = skott::file_tools::FileRequest::new(
        dir.path().to_str().unwrap(),
        "read_file",
        json!({"path":path,"limit":16384}),
    )
    .unwrap();
    let page = skott::file_tools::execute_file_request(&request).unwrap();
    let mut session = make_session();
    session.record_tool_result(
        "page",
        "read_file",
        &ToolOutcome {
            stdout: serde_json::to_vec(&page).unwrap(),
            ..success_outcome("")
        },
    );
    let request = session.next_request().unwrap();
    let ModelMessage::ToolResult { content, .. } = request.messages.last().unwrap() else {
        panic!("tool result");
    };
    let payload = content.split_once('\n').unwrap().1;
    let retained: serde_json::Value = serde_json::from_str(payload).expect("complete native JSON");
    assert_eq!(retained["sha256"], page["sha256"]);
    assert_eq!(
        retained["next_offset"],
        retained["content"].as_str().unwrap().len()
    );
    assert_eq!(retained["total_bytes"], text.len());
    assert!(serde_json::to_vec(content).unwrap().len() <= 8192);
}

#[test]
fn reliability_length_recovery_respects_window_and_shared_token_budgets() {
    for limited_by_window in [false, true] {
        let mut partial = tool_turn("partial", "shell", json!({"command":"echo never"}));
        partial.finish_reason = FinishReason::Length;
        let transport = CapturingTransport::new(vec![partial, answer_turn("not requested")]);
        let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
        let mut session = make_session();
        session.push_user("work");
        let mut request = session.next_request().unwrap();
        request.max_output_tokens = Some(1024);
        let charge = skott::context::estimate_request(&request) + 1024;
        let mut context = ContextManager::for_model(if limited_by_window {
            charge + 512
        } else {
            8192
        });
        let mut recorder = FakeRecorder::default();
        let limits = skott::session::RunLimits {
            max_tokens: if limited_by_window {
                1_000_000
            } else {
                charge as u64 + 64
            },
            ..skott::session::RunLimits::default()
        };
        let result = AgentRunner::default().with_limits(limits).unwrap().run(
            &mut session,
            &transport,
            &executor,
            &Collector::default(),
            &CancellationToken::new(),
            &mut context,
            Some(&mut recorder),
        );
        assert!(result.is_err() || result.is_ok_and(|summary| !summary.success()));
        assert!(executor.executed().is_empty());
        assert_eq!(transport.requests().len(), 1);
    }
}

#[test]
fn reliability_recovery_recording_failure_stops_before_effects() {
    let mut partial = answer_turn("partial");
    partial.finish_reason = FinishReason::Length;
    let transport = CapturingTransport::new(vec![
        partial,
        tool_turn("effect", "shell", json!({"command":"echo must-not-run"})),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("work");
    let mut context = ContextManager::for_model(8192);
    let mut recorder = FakeRecorder {
        fail_notice: true,
        ..FakeRecorder::default()
    };
    let result = AgentRunner::default().run(
        &mut session,
        &transport,
        &executor,
        &Collector::default(),
        &CancellationToken::new(),
        &mut context,
        Some(&mut recorder),
    );
    assert!(result.is_err());
    assert_eq!(transport.requests().len(), 1);
    assert!(executor.executed().is_empty());
}

#[test]
fn reliability_successful_distinct_effect_resets_repeat_stalls() {
    let repeated = tool_turn("same", "shell", json!({"command":"echo a"}));
    let transport = ScriptedTransport::new(vec![
        repeated.clone(),
        repeated.clone(),
        repeated.clone(),
        repeated.clone(),
        tool_turn("distinct", "shell", json!({"command":"echo b"})),
        repeated.clone(),
        repeated,
        answer_turn("recovered"),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("work");
    let mut context = ContextManager::for_model(8192);
    let (summary, _) = run_collected(
        &mut session,
        &transport,
        &executor,
        &CancellationToken::new(),
        &mut context,
        &mut FakeRecorder::default(),
    );
    assert!(summary.success(), "{summary:?}");
    assert_eq!(executor.executed().len(), 3);
}

#[test]
fn hardening_record_failure_precedes_effects_and_closes_unsuccessfully() {
    let transport = ScriptedTransport::new(vec![tool_turn(
        "write",
        "shell",
        json!({"command":"echo effect"}),
    )]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("work");
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder {
        fail_dispatch: true,
        ..FakeRecorder::default()
    };
    let result = AgentRunner::default().run(
        &mut session,
        &transport,
        &executor,
        &Collector::default(),
        &CancellationToken::new(),
        &mut context,
        Some(&mut recorder),
    );
    assert!(result.is_err());
    assert!(executor.executed().is_empty());
    assert_eq!(recorder.terminal_calls.load(Ordering::SeqCst), 1);
    assert!(!recorder.session_finished.load(Ordering::SeqCst));
}

#[test]
fn hardening_transport_failure_closes_exactly_once() {
    let transport = RetryingTransport::new(usize::MAX, answer_turn("never"));
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("work");
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let summary = AgentRunner::with_retry(1, fast_retry())
        .run(
            &mut session,
            &transport,
            &executor,
            &Collector::default(),
            &CancellationToken::new(),
            &mut context,
            Some(&mut recorder),
        )
        .unwrap();
    assert!(summary.failure.is_some());
    assert!(!summary.success());
    assert_eq!(recorder.terminal_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn hardening_token_budget_is_charged_before_any_provider_attempt() {
    let transport = RetryingTransport::new(0, answer_turn("never"));
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("work");
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let summary = AgentRunner::default()
        .with_limits(skott::RunLimits {
            max_tokens: 1,
            ..skott::RunLimits::default()
        })
        .unwrap()
        .run(
            &mut session,
            &transport,
            &executor,
            &Collector::default(),
            &CancellationToken::new(),
            &mut context,
            Some(&mut recorder),
        )
        .unwrap();
    assert!(summary.budget_exhausted);
    assert_eq!(*transport.attempts.lock().unwrap(), 0);
    assert_eq!(recorder.terminal_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn hardening_cancellation_interrupts_backoff_without_another_attempt() {
    let transport = RetryingTransport::new(usize::MAX, answer_turn("never"));
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("work");
    let cancellation = CancellationToken::new();
    let canceller = cancellation.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        canceller.cancel();
    });
    let started = std::time::Instant::now();
    let summary = AgentRunner::with_retry(
        2,
        RetryPolicy::new(2, Duration::from_secs(5), Duration::from_secs(5)),
    )
    .run(
        &mut session,
        &transport,
        &executor,
        &Collector::default(),
        &cancellation,
        &mut ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
        None,
    )
    .unwrap();
    thread.join().unwrap();
    assert!(summary.cancelled);
    assert_eq!(*transport.attempts.lock().unwrap(), 1);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn hardening_shared_deadline_bounds_backoff() {
    let transport = RetryingTransport::new(usize::MAX, answer_turn("never"));
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut session = make_session();
    session.push_user("work");
    let started = std::time::Instant::now();
    let summary = AgentRunner::with_retry(
        2,
        RetryPolicy::new(2, Duration::from_secs(5), Duration::from_secs(5)),
    )
    .with_limits(skott::RunLimits {
        wall_time: Duration::from_millis(50),
        ..skott::RunLimits::default()
    })
    .unwrap()
    .run(
        &mut session,
        &transport,
        &executor,
        &Collector::default(),
        &CancellationToken::new(),
        &mut ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
        None,
    )
    .unwrap();
    assert!(summary.budget_exhausted);
    assert!(!summary.cancelled);
    assert_eq!(*transport.attempts.lock().unwrap(), 1);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_transient_failure_is_retried_until_success() {
    // Fail twice with a temporal timeout, then succeed: the loop should retry
    // with back-off and still deliver the answer, reporting each retry.
    let transport = RetryingTransport::new(2, answer_turn("recovered"));
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("go");
    let summary = AgentRunner::with_retry(MAX_TURNS, fast_retry()).run(
        &mut session,
        &transport,
        &executor,
        &Collector::default(),
        &cancellation,
        &mut context,
        Some(&mut recorder),
    );

    let summary = summary.expect("loop recovers and completes");
    assert_eq!(summary.turns, 1);
    assert_eq!(summary.answer.as_deref(), Some("recovered"));
    // Three attempts total: two failures plus the successful retry.
    assert_eq!(*transport.attempts.lock().unwrap(), 3);
    assert!(recorder.session_finished.load(Ordering::SeqCst));
}

#[test]
fn a_single_turn_cap_limits_one_prompt_to_one_model_turn() {
    // The transport keeps proposing tools, so only the cap stops the loop. With
    // a per-prompt cap of one turn the loop performs a single model turn and one
    // tool, even though more work was queued. A cap of one is the safe default
    // for host execution (where a prompt runs with real privileges); sandboxed
    // work is multi-turn by default, bounded by MAX_TURNS.
    let transport = ScriptedTransport::new(vec![
        tool_turn("plan first", "shell", json!({ "command": "echo hi" })),
        tool_turn("plan again", "shell", json!({ "command": "echo hi" })),
        tool_turn("plan again", "shell", json!({ "command": "echo hi" })),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("go");
    let summary = AgentRunner::with_retry(1, fast_retry())
        .run(
            &mut session,
            &transport,
            &executor,
            &Collector::default(),
            &cancellation,
            &mut context,
            Some(&mut recorder),
        )
        .expect("loop completes within the cap");

    assert_eq!(
        summary.turns, 1,
        "the cap stops the loop after one model turn"
    );
    assert_eq!(
        summary.tools_executed, 1,
        "only the first turn's tool executes"
    );
    assert!(
        summary.exhausted,
        "a prompt cut off before it produced an answer is exhausted"
    );
}

#[test]
fn a_single_turn_that_produces_an_answer_is_a_clean_completion() {
    // A single-turn prompt whose model gives a final answer must be recorded as
    // a clean completion, even though it consumed its one permitted turn.
    let transport = ScriptedTransport::new(vec![answer_turn("all done")]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("go");
    let summary = AgentRunner::with_retry(1, fast_retry())
        .run(
            &mut session,
            &transport,
            &executor,
            &Collector::default(),
            &cancellation,
            &mut context,
            Some(&mut recorder),
        )
        .expect("loop completes within the cap");

    assert_eq!(summary.turns, 1);
    assert_eq!(summary.answer.as_deref(), Some("all done"));
    assert!(!summary.exhausted, "a final answer is a clean completion");
    assert!(!summary.cancelled);
}

#[test]
fn a_retry_is_reported_as_a_note_before_each_attempt() {
    // Each retry must surface an in-progress note so the user sees the back-off
    // rather than a silent hang, even though the turn has not yet completed.
    let transport = RetryingTransport::new(2, answer_turn("recovered"));
    let sink = Collector::default();
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("go");
    let _ = AgentRunner::with_retry(MAX_TURNS, fast_retry())
        .run(
            &mut session,
            &transport,
            &executor,
            &sink,
            &cancellation,
            &mut context,
            Some(&mut recorder),
        )
        .expect("loop recovers and completes");

    // A note is emitted before each of the two retries; the compaction path
    // does not fire here, so every note is a retry notice.
    let notes: Vec<Event> = sink
        .events()
        .iter()
        .filter(|e| matches!(e, Event::Note(_)))
        .cloned()
        .collect();
    assert_eq!(notes.len(), 2, "one retry note per attempted retry");
}

#[test]
fn exhausted_retries_report_the_failure() {
    // Fail more times than the attempt budget allows: the loop gives up after
    // the budget and reports the failure rather than retrying forever.
    let transport = RetryingTransport::new(5, answer_turn("never reached"));
    let sink = Collector::default();
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("go");
    let result = AgentRunner::with_retry(MAX_TURNS, fast_retry()).run(
        &mut session,
        &transport,
        &executor,
        &sink,
        &cancellation,
        &mut context,
        Some(&mut recorder),
    );

    // After exhausting the budget the loop reports the failure as an
    // `Event::Failed` and returns a normal summary (matching the pre-retry
    // contract), rather than aborting the run with an error.
    let summary = result.expect("loop reports failure via Event::Failed");
    assert_eq!(summary.turns, 1);
    assert!(summary.answer.is_none());
    // 1 initial attempt + 2 retries = 3, then it stops.
    assert_eq!(*transport.attempts.lock().unwrap(), 3);
    let events = sink.events();
    assert!(events.iter().any(|e| matches!(e, Event::Failed(_))));
    // No answer is produced when the turn never completes.
    assert!(!events.iter().any(|e| matches!(e, Event::Finished { .. })));
}

#[test]
fn a_non_retryable_failure_is_not_retried() {
    // A permanent failure (cancellation) must not be retried, even though it is
    // a transport error: retrying a user-cancelled turn would be wrong.
    struct CancelOnceTransport {
        attempts: Arc<Mutex<usize>>,
    }

    impl ModelTransport for CancelOnceTransport {
        fn complete(
            &self,
            _request: &ModelRequest,
            _cancellation: &CancellationToken,
        ) -> sav::Result<ModelTurn> {
            Err(sav::Error::ModelTransportCancelled)
        }

        fn stream(
            &self,
            _request: &ModelRequest,
            _cancellation: &CancellationToken,
            _on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            *self.attempts.lock().unwrap() += 1;
            Err(sav::Error::ModelTransportCancelled)
        }

        fn deadline(&self) -> Duration {
            Duration::from_secs(30)
        }
    }

    let transport = CancelOnceTransport {
        attempts: Arc::new(Mutex::new(0)),
    };
    let sink = Collector::default();
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("go");
    let result = AgentRunner::with_retry(MAX_TURNS, fast_retry()).run(
        &mut session,
        &transport,
        &executor,
        &sink,
        &cancellation,
        &mut context,
        Some(&mut recorder),
    );

    // A non-retryable error is surfaced as an `Event::Failed` on the first
    // attempt only: no retry note is emitted, proving the loop did not back off
    // and replay the cancelled turn.
    let summary = result.expect("loop reports the cancellation as a failure");
    assert_eq!(summary.turns, 1);
    assert_eq!(*transport.attempts.lock().unwrap(), 1);
    assert!(sink.events().iter().any(|e| matches!(e, Event::Failed(_))));
    assert!(!sink.events().iter().any(|e| matches!(e, Event::Note(_))));
}

/// A transport that records the per-attempt deadline it was granted and fails
/// its first `failures` attempts with a temporal timeout, forcing the loop to
/// back off and retry. Lets the test observe how the retry loop grows the
/// budget each attempt.
struct DeadlineRecordingTransport {
    failures: usize,
    deadlines: Arc<Mutex<Vec<Duration>>>,
}

impl ModelTransport for DeadlineRecordingTransport {
    fn complete(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
    ) -> sav::Result<ModelTurn> {
        Err(sav::Error::ModelTransportCancelled)
    }

    fn stream(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
        _on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        // The loop calls `stream_with_deadline`, which this transport overrides,
        // so the plain `stream` is never reached.
        Err(sav::Error::ModelTransportCancelled)
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(30)
    }

    fn stream_with_deadline(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
        _on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        deadline: Duration,
    ) -> sav::Result<ModelTurn> {
        let count = {
            let mut recorded = self.deadlines.lock().unwrap();
            recorded.push(deadline);
            recorded.len()
        };
        if count <= self.failures {
            // A temporal, recoverable failure; the loop should retry with a
            // larger budget on the next attempt.
            Err(sav::Error::ModelTransportTimedOut)
        } else {
            Ok(answer_turn("recovered"))
        }
    }
}

#[test]
fn each_retry_grants_a_larger_and_capped_budget() {
    // A turn that runs past one deadline must be granted more time on each retry,
    // so an attempt eventually has room to finish the whole generation. The
    // budget grows linearly with the attempt number and is capped at
    // base × max_attempts.
    let deadlines = Arc::new(Mutex::new(Vec::new()));
    let transport = DeadlineRecordingTransport {
        failures: 3,
        deadlines: deadlines.clone(),
    };
    let sink = Collector::default();
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("go");
    let _ = AgentRunner::with_retry(MAX_TURNS, fast_retry()).run(
        &mut session,
        &transport,
        &executor,
        &sink,
        &cancellation,
        &mut context,
        Some(&mut recorder),
    );

    // The base deadline is 30s with a 3-attempt budget, so attempts get 30s,
    // 60s, then 90s (30 × max_attempts) before giving up.
    assert_eq!(
        *deadlines.lock().unwrap(),
        vec![
            Duration::from_secs(30),
            Duration::from_secs(60),
            Duration::from_secs(90),
        ],
        "each retry grants a larger, capped budget"
    );
}

/// A transport that streams text slowly, crossing the live-progress interval, so
/// the loop must emit at least one progress update while the turn is still
/// streaming (not only after it completes).
struct SlowStreamingTransport {
    turn: ModelTurn,
}

impl ModelTransport for SlowStreamingTransport {
    fn complete(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
    ) -> sav::Result<ModelTurn> {
        Err(sav::Error::ModelTransportCancelled)
    }

    fn stream(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        for _ in 0..4 {
            thread::sleep(Duration::from_millis(130));
            on_event(ModelStreamEvent::TextDelta("word ".to_owned()))?;
        }
        Ok(self.turn.clone())
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(30)
    }
}

#[test]
fn a_streaming_turn_emits_live_progress_mid_turn() {
    // A turn that streams for longer than the live-progress interval must surface
    // progress while it is still generating, so the stats bar tracks the run.
    // Exactly one progress is emitted after the turn ends, so a count of two or
    // more proves a live update fired during streaming.
    let transport = SlowStreamingTransport {
        turn: answer_turn("done"),
    };
    let sink = Collector::default();
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("go");
    AgentRunner::with_retry(MAX_TURNS, fast_retry())
        .run(
            &mut session,
            &transport,
            &executor,
            &sink,
            &cancellation,
            &mut context,
            Some(&mut recorder),
        )
        .expect("loop recovers and completes");

    let progresses = sink
        .events()
        .iter()
        .filter(|e| matches!(e, Event::Progress { .. }))
        .count();
    assert!(
        progresses >= 2,
        "expected live progress during streaming plus the post-turn report, got {progresses}"
    );
}

/// A transport that yields scripted turns while recording every request it is
/// asked to send, so tests can assert the loop never sends a request whose final
/// turn is an unanswered assistant tool call (which a real backend rejects with
/// 400).
struct CapturingTransport {
    turns: Arc<Mutex<Vec<ModelTurn>>>,
    requests: Arc<Mutex<Vec<ModelRequest>>>,
}

impl CapturingTransport {
    fn new(turns: Vec<ModelTurn>) -> Self {
        CapturingTransport {
            turns: Arc::new(Mutex::new(turns)),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl ModelTransport for CapturingTransport {
    fn complete(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
    ) -> sav::Result<ModelTurn> {
        Err(sav::Error::ModelTransportCancelled)
    }

    fn stream(
        &self,
        request: &ModelRequest,
        _cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        self.requests.lock().unwrap().push(request.clone());
        let turn = {
            let mut turns = self.turns.lock().unwrap();
            if turns.is_empty() {
                answer_turn("(no more turns)")
            } else {
                turns.remove(0)
            }
        };
        if let Some(reasoning) = &turn.reasoning {
            on_event(ModelStreamEvent::ReasoningDelta(reasoning.clone()))?;
        }
        if !turn.text.is_empty() {
            on_event(ModelStreamEvent::TextDelta(turn.text.clone()))?;
        }
        for intent in &turn.tool_intents {
            on_event(ModelStreamEvent::ToolIntent(intent.clone()))?;
        }
        Ok(turn)
    }

    fn deadline(&self) -> Duration {
        Duration::from_secs(30)
    }
}

/// Counts assistant tool calls not yet answered by a following tool result.
/// Zero means every proposed tool call has a result, so the model can be asked
/// to continue. A positive count means the last turn left an assistant message
/// with tool_calls unanswered, which is exactly the sequence an OpenAI backend
/// rejects with 400.
fn unanswered_tool_calls(messages: &[ModelMessage]) -> usize {
    let mut pending = 0usize;
    for message in messages {
        match message {
            ModelMessage::Assistant { tool_intents, .. } => pending += tool_intents.len(),
            ModelMessage::ToolResult { .. } => pending = pending.saturating_sub(1),
            _ => {}
        }
    }
    pending
}

#[test]
fn a_denied_tool_still_records_a_tool_result_for_the_next_turn() {
    // A tool the executor refuses must still produce a tool result in the
    // conversation, so the next request never ends on an unanswered assistant
    // tool call. A real OpenAI-compatible backend rejects such a request with
    // 400 "Cannot continue an assistant message that contains tool calls".
    let transport = CapturingTransport::new(vec![
        tool_turn("thinking", "shell", json!({ "command": "rm -rf /" })), // denied by policy
        answer_turn("refused and continued"),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
    session.push_user("please go");
    let runner = AgentRunner::default();
    let sink = Collector::default();
    let summary = runner
        .run(
            &mut session,
            &transport,
            &executor,
            &sink,
            &cancellation,
            &mut context,
            Some(&mut recorder),
        )
        .expect("loop completes without error");

    assert_eq!(summary.turns, 2);
    assert_eq!(summary.tools_executed, 0);
    assert_eq!(summary.answer.as_deref(), Some("refused and continued"));
    // Every request the loop sent kept the assistant->tool sequence valid, so a
    // backend would never reject it.
    for request in transport.requests() {
        assert_eq!(
            unanswered_tool_calls(&request.messages),
            0,
            "request ended on unanswered assistant tool calls"
        );
    }
}
