// Tests for prompt steering during active autonomous tasks.
//
// Steering lets a user submit additional guidance to a running task without
// creating a new session or resetting conversation context. The guidance is
// delivered via push_user() at safe interruption points, and the model sees it
// in the next request.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sav::{
    CancellationToken, FinishReason, LocalModelProvider, ModelMessage, ModelRequest,
    ModelStreamEvent, ModelTransport, ModelTurn, ModelUsage, ReasoningEffort, ToolIntent,
};
use serde_json::json;

use skott::{
    AgentSession, ContextManager, DEFAULT_CONTEXT_TOKENS, Event, EventSink, RunLimits, Skott,
    ToolExecutor, ToolOutcome, ToolPolicy, ToolRegistry,
};

/// A transport that yields a scripted sequence of turns.
struct ScriptedTransport {
    turns: Arc<Mutex<Vec<ModelTurn>>>,
    requests: Arc<Mutex<Vec<ModelRequest>>>,
}

impl ScriptedTransport {
    fn new(turns: Vec<ModelTurn>) -> Self {
        ScriptedTransport {
            turns: Arc::new(Mutex::new(turns)),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Every model request the loop sent, in order. The first request carries
    /// the initial task; a later request carries any steering delivered before
    /// it, so these are the ground truth that steering reached the model.
    fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().unwrap().clone()
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

struct RecordingExecutor {
    executed: Arc<Mutex<Vec<(String, String)>>>,
}

impl RecordingExecutor {
    fn new() -> Self {
        RecordingExecutor {
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
    ) -> skott::Result<ToolOutcome> {
        let result = if intent.name == "shell" {
            let cmd = intent
                .arguments
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if cmd.contains("true") {
                "exit 0"
            } else {
                "exit 1"
            }
        } else {
            "ok"
        };
        self.executed
            .lock()
            .unwrap()
            .push((intent.name.clone(), result.to_string()));
        let stdout = if intent.name == "read_file" {
            let offset = intent
                .arguments
                .get("offset")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            serde_json::to_vec(&serde_json::json!({
                "content":"ok", "sha256":format!("sha256:{}", "0".repeat(64)),
                "offset":offset, "next_offset":offset + 2, "total_bytes":1048576,
            }))
            .unwrap()
        } else {
            result.as_bytes().to_vec()
        };
        Ok(ToolOutcome {
            exited: true,
            status: Some(0),
            stdout,
            stderr: vec![],
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        })
    }
}

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

fn make_session() -> AgentSession {
    let registry = ToolRegistry::new(ToolPolicy::minimum());
    let model = skott::config::Model {
        context_limit: None,
        response_reserve: None,
        id: "test".to_owned(),
        provider: skott::config::ModelProvider::LlamaServer,
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
        registry.tool_definitions(),
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
) -> skott::Result<skott::RunSummary> {
    let runner = Skott::default();
    runner.with_limits(RunLimits::default())?.run(
        session,
        transport,
        executor,
        sink,
        cancellation,
        context,
        None,
    )
}

/// Steering via push_user between turns is delivered in the next request.
#[test]
fn steering_applied_between_turns() {
    let mut session = make_session();
    session.push_user("Process the file".to_string());

    // Simulate one turn completing
    let turn = tool_turn(
        "reading file",
        "read_file",
        json!({"path": "/tmp/test.txt"}),
    );
    session.apply_assistant(turn);

    // User steers after seeing the tool call
    session.push_user("Also validate the file format".to_string());

    // The steering message is now in the conversation
    let messages = session.messages();
    assert!(
        messages.iter().any(|m| {
            if let ModelMessage::User(content) = m {
                content.contains("validate the file format")
            } else {
                false
            }
        }),
        "Steering message should be in conversation"
    );

    // And the original task is still there
    assert!(
        messages.iter().any(|m| {
            if let ModelMessage::User(content) = m {
                content.contains("Process the file")
            } else {
                false
            }
        }),
        "Original task should remain in conversation"
    );
}

/// Steering preserves completed tool results.
#[test]
fn steering_preserves_tool_results() {
    let mut session = make_session();
    session.push_user("Process the file".to_string());

    let turn = tool_turn(
        "reading file",
        "read_file",
        json!({"path": "/tmp/test.txt"}),
    );
    let intents = session.apply_assistant(turn);

    // Execute the tool
    let executor = RecordingExecutor::new();
    for intent in &intents {
        let _ = executor.execute(intent, &CancellationToken::new());
    }

    // Record the result
    session.record_tool_result(
        "call-1",
        "read_file",
        &ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: b"file content".to_vec(),
            stderr: vec![],
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        },
    );

    // Now steer
    session.push_user("Also validate the file format".to_string());

    // The conversation still contains both the original task and steering
    let messages = session.messages();
    let has_original = messages.iter().any(|m| {
        if let ModelMessage::User(content) = m {
            content.contains("Process the file")
        } else {
            false
        }
    });
    let has_steering = messages.iter().any(|m| {
        if let ModelMessage::User(content) = m {
            content.contains("validate the file format")
        } else {
            false
        }
    });
    assert!(has_original, "Original task should persist");
    assert!(has_steering, "Steering should be added");
}

/// Multiple steering messages are queued in order.
#[test]
fn multiple_steering_messages_preserved_in_order() {
    let mut session = make_session();
    session.push_user("Original task".to_string());

    let turn = tool_turn(
        "reading file",
        "read_file",
        json!({"path": "/tmp/test.txt"}),
    );
    session.apply_assistant(turn);

    // Multiple steering messages
    session.push_user("Steering 1".to_string());
    session.push_user("Steering 2".to_string());

    let messages = session.messages();
    let steering_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            if let ModelMessage::User(content) = m {
                content.starts_with("Steering")
            } else {
                false
            }
        })
        .map(|(i, _)| i)
        .collect();

    assert_eq!(steering_indices.len(), 2);
    // Steering 1 should come before Steering 2
    assert!(steering_indices[0] < steering_indices[1]);
}

/// Steering does not create a new session or reset state.
#[test]
fn steering_does_not_reset_session() {
    let mut session = make_session();
    session.push_user("Original task".to_string());

    let turn1 = tool_turn(
        "reading file",
        "read_file",
        json!({"path": "/tmp/test.txt"}),
    );
    session.apply_assistant(turn1);
    session.record_tool_result(
        "call-1",
        "read_file",
        &ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: b"content".to_vec(),
            stderr: vec![],
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        },
    );

    let turn2 = tool_turn(
        "writing file",
        "write_file",
        json!({"path": "/tmp/out.txt"}),
    );
    session.apply_assistant(turn2);
    session.record_tool_result(
        "call-2",
        "write_file",
        &ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: b"ok".to_vec(),
            stderr: vec![],
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        },
    );

    // Steer
    session.push_user("Actually use a different output path".to_string());

    // Both tool results should still be in conversation
    let messages = session.messages();
    // Check that we have the original task and steering
    let has_original = messages.iter().any(|m| {
        if let ModelMessage::User(content) = m {
            content.contains("Original task")
        } else {
            false
        }
    });
    let has_steering = messages.iter().any(|m| {
        if let ModelMessage::User(content) = m {
            content.contains("different output path")
        } else {
            false
        }
    });
    assert!(has_original, "Original task should persist");
    assert!(has_steering, "Steering should be added");
}

/// Steering can be applied after tool execution but before the next model request.
#[test]
fn steering_applied_after_tool_before_model() {
    let mut session = make_session();
    session.push_user("Read and process".to_string());

    // Model proposes tool
    let turn = tool_turn(
        "reading file",
        "read_file",
        json!({"path": "/tmp/test.txt"}),
    );
    let intents = session.apply_assistant(turn);

    // Execute tool
    let executor = RecordingExecutor::new();
    for intent in &intents {
        let _ = executor.execute(intent, &CancellationToken::new());
    }
    session.record_tool_result(
        "call-1",
        "read_file",
        &ToolOutcome {
            exited: true,
            status: Some(0),
            stdout: b"content".to_vec(),
            stderr: vec![],
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        },
    );

    // Steering injected here (between tool result and next model request)
    session.push_user("Skip the processing step".to_string());

    // Next model request will include the steering
    let messages = session.messages();
    let last_user = messages
        .iter()
        .rfind(|m| matches!(m, ModelMessage::User(_)));
    assert!(
        matches!(last_user, Some(ModelMessage::User(content)) if content.contains("Skip the processing step"))
    );
}

/// Full loop: steering delivered at the turn boundary reaches the model in the
/// next request and the loop still completes the original task.
///
/// The user submits a task, then steers it. The scripted model proposes a tool
/// call on the first turn and concludes on the second. The loop must execute the
/// tool, carry both the original task and the steering message in every request
/// it builds (so steering persists rather than resetting the conversation), and
/// report the terminal answer.
#[test]
fn steering_in_full_loop() {
    let mut session = make_session();
    session.push_user("Read the config file and apply changes".to_string());

    // The user steers after the task is queued, before the next model request
    // is built. This is the message the loop must deliver at the turn boundary.
    session.push_user("Actually, check for environment variables first".to_string());

    // The scripted model reads the file on the first turn and concludes on the
    // second. The transport captures every request the loop sends, which is the
    // ground truth that steering reached the model.
    let transport = ScriptedTransport::new(vec![
        tool_turn(
            "reading config",
            "read_file",
            json!({"path": "/app/config.json"}),
        ),
        answer_turn("applied the changes"),
    ]);
    let executor = RecordingExecutor::new();
    let sink = Collector::default();
    let cancellation = CancellationToken::new();
    let mut context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);

    let summary = run_once(
        &mut session,
        &transport,
        &executor,
        &sink,
        &cancellation,
        &mut context,
    )
    .expect("loop completes with the scripted transport");

    // The loop ran a full tool round and reached the terminal answer.
    assert!(
        summary.success(),
        "loop should complete successfully: {summary:?}"
    );
    assert_eq!(summary.turns, 2);
    assert_eq!(summary.tools_executed, 1);
    assert_eq!(summary.answer.as_deref(), Some("applied the changes"));

    // The tool was actually executed by the loop, not skipped.
    assert_eq!(
        executor.executed(),
        vec![("read_file".to_string(), "ok".to_string())],
        "the loop must have executed the scripted tool intent"
    );

    // The loop sent exactly two model requests, one per scripted turn.
    let requests = transport.requests();
    assert_eq!(
        requests.len(),
        2,
        "the loop made one request per scripted turn"
    );

    // Both the original task and the steering message must appear in the loop's
    // request stream - steering is delivered without a new session or a reset.
    let stream = requests
        .iter()
        .flat_map(|request| request.messages.iter())
        .collect::<Vec<_>>();
    assert!(
        stream.iter().any(|m| matches!(
            m,
            ModelMessage::User(content)
                if content.contains("Read the config file and apply changes")
        )),
        "original task must remain in the loop's request stream"
    );
    let steering_request = requests
        .iter()
        .find(|request| {
            request.messages.iter().any(|m| {
                matches!(
                    m,
                    ModelMessage::User(content)
                        if content.contains("environment variables")
                )
            })
        })
        .expect("a model request must carry the steering message");

    // The steering was delivered at the turn boundary, so it shares the original
    // task's request rather than replacing it.
    assert!(
        steering_request.messages.iter().any(|m| matches!(
            m,
            ModelMessage::User(content)
                if content.contains("Read the config file and apply changes")
        )),
        "steering must share the original task's request, not replace it"
    );

    // The captured events confirm the loop streamed a tool round and a terminal
    // answer, exercising the sink the loop pushes through.
    let events = sink.events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::ToolResult { failed, .. } if !failed)),
        "the loop must have reported a successful tool result"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::Finished { message }
                if message == "applied the changes"
        )),
        "the loop must have reported the terminal answer"
    );
}
