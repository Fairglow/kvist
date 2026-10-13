//! Explicit local qualification; never part of the offline default test suite.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use sav::{
    CancellationToken, ModelMessage, ModelRequest, ModelStreamEvent, ModelTransport,
    ReasoningEffort, ToolChoice, ToolIntent,
};
use skott::{
    AgentSession, ContextManager, Event, EventSink, Model, ModelProvider, RunLimits,
    SandboxExecutor, SandboxPaths, SessionLog, Skott, ToolPolicy, ToolRegistry,
};

#[test]
#[ignore = "requires an independently installed native sandbox runner"]
fn real_sandbox_confines_native_files_and_denies_host_network() {
    use skott::ToolExecutor;
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("work");
    std::fs::create_dir(&workspace).unwrap();
    let outside = root.path().join("outside.txt");
    std::fs::write(&outside, "outside-sentinel").unwrap();
    let registry = ToolRegistry::new(ToolPolicy::minimum())
        .with_file_helper(env!("CARGO_BIN_EXE_skott-file-tool"));
    let executor = SandboxExecutor::new(
        registry,
        SandboxPaths {
            runner: std::env::var_os("KVIST_LIVE_GALLA_RUNNER")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/usr/local/bin/galla-runner")),
            backend: PathBuf::from("/usr/bin/bwrap"),
        },
        workspace.clone(),
    );
    let invoke = |name: &str, arguments| {
        executor
            .execute(
                &ToolIntent {
                    id: "qualification".into(),
                    provider_id: None,
                    name: name.into(),
                    arguments,
                },
                &CancellationToken::new(),
            )
            .unwrap()
    };
    let written = invoke(
        "write_file",
        serde_json::json!({"path":"/workspace/file.txt","content":"confined"}),
    );
    assert_eq!(written.status, Some(0), "{}", written.error_text(1000));
    let read = invoke(
        "read_file",
        serde_json::json!({"path":"/workspace/file.txt"}),
    );
    assert_eq!(read.status, Some(0));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&read.stdout).unwrap()["content"],
        "confined"
    );
    let denied = invoke("read_file", serde_json::json!({"path":outside}));
    assert_ne!(denied.status, Some(0));
    assert!(!denied.output_text(1000).contains("outside-sentinel"));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let network = invoke(
        "shell",
        serde_json::json!({"command":format!(
            "if {{ exec 3<>/dev/tcp/127.0.0.1/{port}; }} 2>/dev/null; then printf 'connected\\n'; else printf 'network-denied\\n'; fi"
        )}),
    );
    assert_eq!(network.status, Some(0), "{}", network.error_text(1000));
    assert_eq!(network.output_text(1000), "network-denied\n");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(std::fs::read_dir(&workspace).unwrap().count(), 1);
}

fn model() -> Model {
    Model {
        id: "live".into(),
        provider: ModelProvider::LlamaServer,
        base_url: std::env::var("KVIST_LIVE_LLAMA_ENDPOINT")
            .unwrap_or_else(|_| "http://127.0.0.1:9931".into()),
        model: std::env::var("KVIST_LIVE_LLAMA_MODEL")
            .unwrap_or_else(|_| "Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL".into()),
        is_default: false,
        deadline_secs: 120,
        context_limit: None,
        response_reserve: None,
        max_attempts: 2,
        retry_base_delay_secs: 1,
        retry_max_delay_secs: 2,
        cadence_timeout_secs: 15,
    }
}

#[derive(Default)]
struct Events(Mutex<Vec<Event>>);
impl EventSink for Events {
    fn send(&self, event: Event) -> skott::Result<()> {
        self.0.lock().unwrap().push(event);
        Ok(())
    }
}

#[test]
#[ignore = "requires an explicitly selected live local llama-server"]
fn live_llama_streams_a_bounded_answer_without_tools() {
    let model = model();
    let transport = model.transport().unwrap();
    let request = ModelRequest {
        model: model.model.clone(),
        messages: vec![ModelMessage::User("Reply with exactly: OK".into())],
        tools: vec![],
        tool_choice: ToolChoice::None,
        reasoning_effort: Some(ReasoningEffort::None),
        output_schema: None,
        max_output_tokens: Some(64),
    };
    let mut streamed = String::new();
    let turn = transport
        .stream(&request, &CancellationToken::new(), &mut |event| {
            if let ModelStreamEvent::TextDelta(text) = event {
                streamed.push_str(&text);
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(turn.text.trim(), "OK");
    assert_eq!(streamed, turn.text);
    assert!(turn.tool_intents.is_empty());
    assert_eq!(turn.finish_reason, sav::FinishReason::Stop);
    if let Some(usage) = turn.usage {
        assert!(usage.output_tokens <= 64);
    }
}

#[test]
#[ignore = "requires live llama-server and an independently installed native sandbox runner"]
fn live_llama_reads_edits_and_verifies_inside_the_real_sandbox() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("work");
    std::fs::create_dir(&workspace).unwrap();
    let target = workspace.join("answer.txt");
    std::fs::write(&target, "answer = 41\r\nunchanged = yes").unwrap();
    let sandbox = SandboxPaths {
        runner: std::env::var_os("KVIST_LIVE_GALLA_RUNNER")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/local/bin/galla-runner")),
        backend: PathBuf::from("/usr/bin/bwrap"),
    };
    let registry = ToolRegistry::resolve(
        ToolPolicy::minimum(),
        &BTreeMap::new(),
        &skott::toolchain::HostProbe,
        None,
    )
    .unwrap()
    .with_file_helper(env!("CARGO_BIN_EXE_skott-file-tool"));
    let definitions = registry.tool_definitions();
    let executor = SandboxExecutor::new(registry, sandbox, workspace.clone());
    let model = model();
    let transport = model.transport().unwrap();
    let mut session = AgentSession::new(
        model.clone(),
        ReasoningEffort::None,
        definitions,
        skott::system_prompt("/workspace"),
    );
    session.push_user(
        "Use read_file on /workspace/answer.txt to obtain its sha256. Then use edit_file \
         with that expected_sha256 to replace exactly 'answer = 41' with 'answer = 42'. \
         Preserve all other bytes, including CRLF and the missing final newline. \
         Read the file again to verify. Do not use shell or write_file. Finally reply DONE.",
    );
    let sink = Events::default();
    let mut log = SessionLog::open(&root.path().join("logs"), "live-isolated-edit").unwrap();
    let summary = Skott::with_retry(8, model.retry_policy())
        .with_limits(RunLimits {
            wall_time: Duration::from_secs(180),
            max_tokens: 100_000,
            response_reserve: 1024,
        })
        .unwrap()
        .run(
            &mut session,
            &transport,
            &executor,
            &sink,
            &CancellationToken::new(),
            &mut ContextManager::new(8192, 2),
            Some(&mut log),
        )
        .unwrap();
    assert!(summary.success(), "{summary:?}");
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"answer = 42\r\nunchanged = yes"
    );
    let events = sink.0.lock().unwrap();
    assert!(events.iter().any(
        |event| matches!(event, Event::ToolResult { name, failed:false, .. } if name == "edit_file")
    ));
    assert!(events.iter().filter(|event| matches!(event, Event::ToolResult { name, failed:false, .. } if name == "read_file")).count() >= 2);
    assert!(!events.iter().any(|event| matches!(event, Event::ToolResult { name, .. } if name == "shell" || name == "write_file")));
    assert_eq!(
        std::fs::read_dir(workspace).unwrap().count(),
        1,
        "no staging files in workspace"
    );
}
