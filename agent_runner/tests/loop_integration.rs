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
use std::time::Duration;

use agent_runtime::{
    CancellationToken, FinishReason, LocalModelProvider, ModelRequest, ModelStreamEvent,
    ModelTransport, ModelTurn, ModelUsage, ReasoningEffort, ToolIntent,
};
use serde_json::json;

use agent_runner::{
    AgentRunner, AgentSession, ContextManager, DEFAULT_CONTEXT_TOKENS, Error, Event, EventSink,
    Model, ModelProvider, Recorder, RunSummary, ToolExecutor, ToolOutcome, ToolPolicy,
    ToolRegistry,
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
    ) -> agent_runtime::Result<ModelTurn> {
        Err(agent_runtime::Error::ModelTransportCancelled)
    }

    fn stream(
        &self,
        _request: &ModelRequest,
        _cancellation: &CancellationToken,
        on_event: &mut dyn FnMut(ModelStreamEvent) -> agent_runtime::Result<()>,
    ) -> agent_runtime::Result<ModelTurn> {
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
}

impl RecordingExecutor {
    fn new(policy: ToolPolicy, workdir: PathBuf) -> Self {
        RecordingExecutor {
            policy,
            workdir,
            executed: Arc::new(Mutex::new(Vec::new())),
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
    ) -> agent_runner::Result<ToolOutcome> {
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
    }
}

/// Collects every event the loop emits so tests can assert on the stream.
#[derive(Default)]
struct Collector {
    events: Arc<Mutex<Vec<Event>>>,
}

impl EventSink for Collector {
    fn send(&self, event: Event) -> agent_runner::Result<()> {
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
}

impl Recorder for FakeRecorder {
    fn session_start(&mut self) {
        self.session_started.store(true, Ordering::SeqCst);
    }

    fn turn_start(&mut self, turn: &ModelTurn) -> usize {
        let index = self.turns.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(reasoning) = &turn.reasoning {
            self.reasoning.lock().unwrap().push(reasoning.clone());
        }
        index
    }

    fn turn_finish(&mut self, _turn: usize, _usage: &Option<ModelUsage>, _finish_reason: &str) {}

    fn tool_result(
        &mut self,
        turn: usize,
        _call_id: &str,
        tool: &str,
        _args: &serde_json::Value,
        outcome: &ToolOutcome,
    ) {
        self.tool_results
            .lock()
            .unwrap()
            .push(format!("turn {turn} {tool} failed={}", outcome.failed()));
    }

    fn session_finish(&mut self, success: bool) {
        self.session_finished.store(success, Ordering::SeqCst);
    }
}

impl FakeRecorder {
    fn reasoning(&self) -> Vec<String> {
        self.reasoning.lock().unwrap().clone()
    }
}

fn make_session() -> AgentSession {
    let model = Model {
        id: "test".to_owned(),
        provider: ModelProvider::LlamaServer,
        base_url: "http://127.0.0.1:9931".to_owned(),
        model: "test-model".to_owned(),
        deadline_secs: 30,
    };
    let tool_defs = ToolRegistry::new(ToolPolicy::minimum()).tool_definitions();
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
            .any(|e| matches!(e, Event::ToolCall { name } if name == "shell"))
    );
    assert!(events.iter().any(|e| matches!(
        e,
        Event::ToolResult { name, failed } if name == "shell" && !failed
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
        Event::ToolResult { name, failed } if name == "shell" && *failed
    )));
    assert!(events.iter().any(|e| matches!(e, Event::Failed(_))));
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
        answer_turn("finished first"),
        tool_turn(
            "working on the second task and reporting back now",
            "shell",
            json!({ "command": "echo second task output" }),
        ),
        answer_turn("finished second"),
        tool_turn(
            "working on the third task and reporting back now",
            "shell",
            json!({ "command": "echo third task output" }),
        ),
        answer_turn("finished third"),
    ]);
    let executor = RecordingExecutor::new(ToolPolicy::minimum(), PathBuf::from("/tmp"));
    // 160-token window, warm-up at 120, keep the most recent turn in full.
    let mut context = ContextManager::new(160, 1);
    let mut recorder = FakeRecorder::default();
    let cancellation = CancellationToken::new();

    let mut session = make_session();
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
            tokens_per_sec,
            context_utilization,
            compaction_progress,
            ..
        } = event
        {
            assert!(context_limit > 0);
            assert!(tokens_per_sec >= 0.0);
            assert!(context_utilization >= 0.0);
            assert!((0.0..=1.0).contains(&compaction_progress));
        }
    }
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
    assert!(events.iter().any(|e| matches!(e, Event::TurnStart { .. })));
    assert!(!events.iter().any(|e| matches!(e, Event::ToolResult { .. })));
}
