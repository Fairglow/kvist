#![cfg(feature = "rig-transport")]

use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use agent_runtime::{
    CancellationToken, FinishReason, LocalModelProvider, ModelMessage, ModelRequest,
    ModelStreamEvent, ModelTransport, RigModelTransport, ToolChoice, ToolDefinition,
};
use serde_json::{Value, json};
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

struct CapturedLogWriter(Arc<Mutex<Vec<u8>>>);

impl<'writer> MakeWriter<'writer> for CapturedLogs {
    type Writer = CapturedLogWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        CapturedLogWriter(Arc::clone(&self.0))
    }
}

impl Write for CapturedLogWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("capture log lock")
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct CapturedRequest {
    head: String,
    body: Value,
}

fn serve_once(response_parts: Vec<Vec<u8>>) -> (String, mpsc::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake provider");
    let address = listener.local_addr().expect("fake provider address");
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");

        let mut request = Vec::new();
        let header_end = loop {
            let mut buffer = [0_u8; 1024];
            let count = stream.read(&mut buffer).expect("read request");
            assert!(count > 0, "request ended before headers");
            request.extend_from_slice(&buffer[..count]);
            if let Some(index) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let head = String::from_utf8(request[..header_end].to_vec()).expect("UTF-8 request head");
        let content_length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .expect("content length");
        while request.len() < header_end + content_length {
            let mut buffer = [0_u8; 1024];
            let count = stream.read(&mut buffer).expect("read request body");
            assert!(count > 0, "request body ended early");
            request.extend_from_slice(&buffer[..count]);
        }
        let body = serde_json::from_slice(&request[header_end..header_end + content_length])
            .expect("JSON request");
        let _ = sender.send(CapturedRequest { head, body });

        for part in response_parts {
            stream.write_all(&part).expect("write fake response");
            stream.flush().expect("flush fake response");
            thread::sleep(Duration::from_millis(5));
        }
    });

    (format!("http://{address}"), receiver)
}

fn json_response(value: Value) -> Vec<Vec<u8>> {
    let body = serde_json::to_vec(&value).expect("serialize response");
    vec![
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes(),
        body,
    ]
}

fn stream_response(records: &[&str]) -> Vec<Vec<u8>> {
    let body = records.concat();
    let mut parts = vec![
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes(),
    ];
    parts.extend(records.iter().map(|record| record.as_bytes().to_vec()));
    parts
}

fn request(tool_choice: ToolChoice) -> ModelRequest {
    ModelRequest {
        model: "test-model".to_owned(),
        messages: vec![
            ModelMessage::System("Follow the contract".to_owned()),
            ModelMessage::User("Read the approved file".to_owned()),
            ModelMessage::ToolResult {
                call_id: "previous-call".to_owned(),
                name: "workspace.read".to_owned(),
                content: "approved content".to_owned(),
            },
        ],
        tools: vec![ToolDefinition {
            name: "workspace.read".to_owned(),
            description: "Read an approved workspace file".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
                "additionalProperties": false
            }),
        }],
        tool_choice,
    }
}

fn transport(provider: LocalModelProvider, endpoint: &str) -> RigModelTransport {
    RigModelTransport::new(provider, endpoint, Duration::from_secs(2), 1024 * 1024)
        .expect("construct Rig transport")
}

#[test]
fn rig_transport_rejects_non_numeric_loopback_endpoints() {
    for endpoint in [
        "https://127.0.0.1:11434",
        "http://localhost:11434",
        "http://example.com:11434",
        "http://user@127.0.0.1:11434",
        "http://127.0.0.1:11434/api/chat",
    ] {
        let error = RigModelTransport::new(
            LocalModelProvider::Ollama,
            endpoint,
            Duration::from_secs(1),
            1024,
        )
        .expect_err("reject endpoint outside the pinned loopback contract");
        assert!(error.to_string().contains("endpoint"));
    }
}

#[test]
fn rig_ollama_unary_maps_canonical_messages_and_text() {
    let (endpoint, captured) = serve_once(json_response(json!({
        "model": "qwen3",
        "created_at": "2026-08-30T00:00:00Z",
        "message": {"role": "assistant", "content": "done", "tool_calls": []},
        "done": true,
        "done_reason": "stop",
        "prompt_eval_count": 10,
        "eval_count": 4
    })));
    let transport = transport(LocalModelProvider::Ollama, &endpoint);

    let turn = transport
        .complete(&request(ToolChoice::Auto), &CancellationToken::new())
        .expect("complete through Rig");

    assert_eq!(turn.text, "done");
    assert_eq!(turn.model, "qwen3");
    assert_eq!(turn.provider, LocalModelProvider::Ollama);
    assert_eq!(turn.finish_reason, FinishReason::Stop);
    assert_eq!(turn.usage.expect("usage").total_tokens, 14);

    let captured = captured.recv().expect("captured request");
    assert!(captured.head.starts_with("POST /api/chat "));
    assert_eq!(captured.body["model"], "test-model");
    assert_eq!(captured.body["messages"][0]["role"], "system");
    assert_eq!(captured.body["messages"][2]["role"], "tool");
    assert_eq!(
        captured.body["tools"][0]["function"]["name"],
        "workspace.read"
    );
}

#[test]
fn rig_llama_server_unary_returns_untrusted_tool_intents() {
    let (endpoint, captured) = serve_once(json_response(json!({
        "id": "chatcmpl-1",
        "model": "server-model",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "I will read it.",
                "tool_calls": [{
                    "id": "call-1",
                    "type": "function",
                    "function": {
                        "name": "workspace.read",
                        "arguments": "{\"path\":\"REQUIREMENTS.md\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 12,
            "completion_tokens": 7,
            "total_tokens": 19
        }
    })));
    let transport = transport(LocalModelProvider::LlamaServer, &endpoint);

    let turn = transport
        .complete(&request(ToolChoice::Required), &CancellationToken::new())
        .expect("complete through Rig");

    assert_eq!(turn.text, "I will read it.");
    assert_eq!(turn.finish_reason, FinishReason::ToolCalls);
    assert_eq!(turn.response_id.as_deref(), Some("chatcmpl-1"));
    assert_eq!(turn.provider_request_id, None);
    assert_eq!(turn.tool_intents.len(), 1);
    assert_eq!(turn.tool_intents[0].id, "call-1");
    assert_eq!(turn.tool_intents[0].provider_id.as_deref(), Some("call-1"));
    assert_eq!(turn.tool_intents[0].name, "workspace.read");
    assert_eq!(
        turn.tool_intents[0].arguments,
        json!({"path": "REQUIREMENTS.md"})
    );

    let captured = captured.recv().expect("captured request");
    assert!(captured.head.starts_with("POST /v1/chat/completions "));
    assert_eq!(captured.body["tool_choice"], "required");
}

#[test]
fn rig_llama_server_stream_emits_text_and_complete_tool_intents() {
    let records = [
        "data: {\"id\":\"chatcmpl-stream\",\"model\":\"server-model\",\"choices\":[{\"delta\":{\"content\":\"Read\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-stream\",\"model\":\"server-model\",\"choices\":[{\"delta\":{\"content\":\"ing\",\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"workspace.read\",\"arguments\":\"{\\\"path\\\":\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-stream\",\"model\":\"server-model\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"REQUIREMENTS.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n",
        "data: [DONE]\n\n",
    ];
    let (endpoint, _) = serve_once(stream_response(&records));
    let transport = transport(LocalModelProvider::LlamaServer, &endpoint);
    let mut events = Vec::new();

    let turn = transport
        .stream(
            &request(ToolChoice::Auto),
            &CancellationToken::new(),
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .expect("stream through Rig");

    assert_eq!(turn.text, "Reading");
    assert_eq!(turn.finish_reason, FinishReason::ToolCalls);
    assert_eq!(turn.usage.expect("usage").total_tokens, 5);
    assert_eq!(
        events,
        vec![
            ModelStreamEvent::TextDelta("Read".to_owned()),
            ModelStreamEvent::TextDelta("ing".to_owned()),
            ModelStreamEvent::ToolIntent(turn.tool_intents[0].clone()),
        ]
    );
}

#[test]
fn rig_transport_honors_preflight_cancellation() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused endpoint");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    let transport = transport(LocalModelProvider::Ollama, &endpoint);
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = transport
        .complete(&request(ToolChoice::None), &cancellation)
        .expect_err("cancel before network access");

    assert!(error.to_string().contains("cancelled"));
}

#[test]
fn rig_transport_rejects_declared_oversized_body_before_reading_it() {
    let response = vec![
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n"
            .to_vec(),
    ];
    let (endpoint, _) = serve_once(response);
    let transport = RigModelTransport::new(
        LocalModelProvider::Ollama,
        &endpoint,
        Duration::from_secs(2),
        128,
    )
    .expect("construct bounded transport");

    let error = transport
        .complete(&request(ToolChoice::None), &CancellationToken::new())
        .expect_err("reject oversized provider body");

    assert!(error.to_string().contains("128-byte response limit"));
}

#[test]
fn rig_transport_deadline_interrupts_a_stalled_provider() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stalled provider");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    thread::spawn(move || {
        let (_stream, _) = listener.accept().expect("accept stalled request");
        thread::sleep(Duration::from_secs(1));
    });
    let transport = RigModelTransport::new(
        LocalModelProvider::Ollama,
        &endpoint,
        Duration::from_millis(30),
        1024,
    )
    .expect("construct timed transport");

    let error = transport
        .complete(&request(ToolChoice::None), &CancellationToken::new())
        .expect_err("deadline must interrupt provider");

    assert!(error.to_string().contains("timed out"));
}

#[test]
fn model_cli_selects_the_optional_rig_transport() {
    let (endpoint, _) = serve_once(json_response(json!({
        "model": "qwen3",
        "created_at": "2026-08-30T00:00:00Z",
        "message": {"role": "assistant", "content": "from Rig", "tool_calls": []},
        "done": true,
        "done_reason": "stop",
        "prompt_eval_count": 1,
        "eval_count": 2
    })));

    let output = Command::new(env!("CARGO_BIN_EXE_agent-run"))
        .args([
            "model",
            "--transport",
            "rig",
            "--provider",
            "ollama",
            "--endpoint",
            &endpoint,
            "--model",
            "test-model",
            "hello",
        ])
        .output()
        .expect("run model command with Rig");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "from Rig\n");
}

#[test]
fn rig_ollama_preserves_tool_choice_without_silent_downgrade() {
    let (endpoint, captured) = serve_once(json_response(json!({
        "model": "qwen3",
        "created_at": "2026-08-30T00:00:00Z",
        "message": {"role": "assistant", "content": "no tools", "tool_calls": []},
        "done": true,
        "done_reason": "stop"
    })));
    let no_tools_transport = transport(LocalModelProvider::Ollama, &endpoint);
    no_tools_transport
        .complete(&request(ToolChoice::None), &CancellationToken::new())
        .expect("complete without exposing tools");
    let captured = captured.recv().expect("captured request");
    assert!(captured.body.get("tools").is_none());
    assert!(captured.body.get("tool_choice").is_none());

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused endpoint");
    listener
        .set_nonblocking(true)
        .expect("make unused endpoint nonblocking");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    let required_transport = transport(LocalModelProvider::Ollama, &endpoint);
    let error = required_transport
        .complete(&request(ToolChoice::Required), &CancellationToken::new())
        .expect_err("reject unsupported required choice");
    assert!(error.to_string().contains("unsupported capability"));
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

#[test]
fn rig_stream_checks_deadline_after_callback_returns() {
    let records = [
        "data: {\"id\":\"chatcmpl-stream\",\"model\":\"server-model\",\"choices\":[{\"delta\":{\"content\":\"slow\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-stream\",\"model\":\"server-model\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    ];
    let (endpoint, _) = serve_once(stream_response(&records));
    let transport = RigModelTransport::new(
        LocalModelProvider::LlamaServer,
        &endpoint,
        Duration::from_millis(20),
        1024 * 1024,
    )
    .expect("construct Rig transport");

    let error = transport
        .stream(
            &request(ToolChoice::None),
            &CancellationToken::new(),
            &mut |_| {
                thread::sleep(Duration::from_millis(50));
                Ok(())
            },
        )
        .expect_err("deadline must be observed when the callback returns");

    assert!(error.to_string().contains("timed out"));
}

#[test]
fn rig_transport_rejects_oversized_serialized_request_before_network() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused endpoint");
    listener
        .set_nonblocking(true)
        .expect("make unused endpoint nonblocking");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    let transport = transport(LocalModelProvider::LlamaServer, &endpoint);
    let mut oversized = request(ToolChoice::Auto);
    oversized.tools[0].parameters = json!({
        "type": "object",
        "description": "x".repeat(2 * 1024 * 1024)
    });

    let error = transport
        .complete(&oversized, &CancellationToken::new())
        .expect_err("reject request above the serialized bound");

    assert!(error.to_string().contains("serialized"));
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

#[test]
fn rig_ollama_stream_preserves_response_limit_classification() {
    let content = "x".repeat(512);
    let record = format!(
        "{{\"model\":\"qwen3\",\"created_at\":\"2026-08-30T00:00:00Z\",\"message\":{{\"role\":\"assistant\",\"content\":\"{content}\"}},\"done\":false}}\n"
    );
    let response = vec![
        b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n"
            .to_vec(),
        record.into_bytes(),
    ];
    let (endpoint, _) = serve_once(response);
    let transport = RigModelTransport::new(
        LocalModelProvider::Ollama,
        &endpoint,
        Duration::from_secs(2),
        128,
    )
    .expect("construct bounded transport");

    let error = transport
        .stream(
            &request(ToolChoice::None),
            &CancellationToken::new(),
            &mut |_| Ok(()),
        )
        .expect_err("reject oversized streamed response");

    assert!(error.to_string().contains("128-byte response limit"));
}

#[test]
fn rig_tool_result_reuses_the_provider_call_identity() {
    let (endpoint, captured) = serve_once(json_response(json!({
        "id": "chatcmpl-2",
        "model": "server-model",
        "choices": [{
            "message": {"role": "assistant", "content": "complete"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    })));
    let transport = transport(LocalModelProvider::LlamaServer, &endpoint);
    let request = ModelRequest {
        model: "test-model".to_owned(),
        messages: vec![
            ModelMessage::Assistant {
                text: String::new(),
                tool_intents: vec![agent_runtime::ToolIntent {
                    id: "canonical-call".to_owned(),
                    provider_id: Some("provider-call".to_owned()),
                    name: "workspace.read".to_owned(),
                    arguments: json!({"path": "REQUIREMENTS.md"}),
                }],
            },
            ModelMessage::ToolResult {
                call_id: "canonical-call".to_owned(),
                name: "workspace.read".to_owned(),
                content: "contents".to_owned(),
            },
        ],
        tools: vec![],
        tool_choice: ToolChoice::None,
    };

    transport
        .complete(&request, &CancellationToken::new())
        .expect("complete replayed tool exchange");

    let captured = captured.recv().expect("captured request");
    assert_eq!(
        captured.body["messages"][0]["tool_calls"][0]["id"],
        "provider-call"
    );
    assert_eq!(
        captured.body["messages"][1]["tool_call_id"],
        "provider-call"
    );
}

#[test]
fn rig_transport_rejects_unbounded_provider_metadata() {
    let (endpoint, _) = serve_once(json_response(json!({
        "model": "x".repeat(257),
        "created_at": "2026-08-30T00:00:00Z",
        "message": {"role": "assistant", "content": "done", "tool_calls": []},
        "done": true,
        "done_reason": "stop"
    })));
    let transport = transport(LocalModelProvider::Ollama, &endpoint);

    let error = transport
        .complete(&request(ToolChoice::None), &CancellationToken::new())
        .expect_err("reject unbounded provider model identity");

    assert!(error.to_string().contains("provider response"));
}

#[test]
fn rig_payload_sentinels_do_not_reach_the_callers_tracing_subscriber() {
    const PROMPT_SENTINEL: &str = "KVIST_PRIVATE_PROMPT_91c30d";
    const RESPONSE_SENTINEL: &str = "KVIST_PRIVATE_RESPONSE_440e55";
    let (endpoint, _) = serve_once(json_response(json!({
        "model": "qwen3",
        "created_at": "2026-08-30T00:00:00Z",
        "message": {
            "role": "assistant",
            "content": RESPONSE_SENTINEL,
            "tool_calls": []
        },
        "done": true,
        "done_reason": "stop"
    })));
    let transport = transport(LocalModelProvider::Ollama, &endpoint);
    let request = ModelRequest {
        model: "test-model".to_owned(),
        messages: vec![ModelMessage::User(PROMPT_SENTINEL.to_owned())],
        tools: Vec::new(),
        tool_choice: ToolChoice::None,
    };
    let captured = CapturedLogs::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(captured.clone())
        .finish();

    let turn = tracing::subscriber::with_default(subscriber, || {
        transport.complete(&request, &CancellationToken::new())
    })
    .expect("complete with caller tracing enabled");

    assert_eq!(turn.text, RESPONSE_SENTINEL);
    let logs = captured.0.lock().expect("read captured logs");
    let logs = String::from_utf8_lossy(&logs);
    assert!(!logs.contains(PROMPT_SENTINEL));
    assert!(!logs.contains(RESPONSE_SENTINEL));
}

#[test]
fn rig_ollama_rejects_nonterminal_unary_responses() {
    let (endpoint, _) = serve_once(json_response(json!({
        "model": "qwen3",
        "created_at": "2026-08-30T00:00:00Z",
        "message": {"role": "assistant", "content": "partial", "tool_calls": []},
        "done": false,
        "prompt_eval_count": 1,
        "eval_count": 1
    })));
    let transport = transport(LocalModelProvider::Ollama, &endpoint);

    let error = transport
        .complete(&request(ToolChoice::None), &CancellationToken::new())
        .expect_err("reject nonterminal unary payload");

    assert!(error.to_string().contains("malformed"));
}

#[test]
fn rig_ollama_rejects_usage_overflow_without_panicking() {
    let (endpoint, _) = serve_once(json_response(json!({
        "model": "qwen3",
        "created_at": "2026-08-30T00:00:00Z",
        "message": {"role": "assistant", "content": "done", "tool_calls": []},
        "done": true,
        "done_reason": "stop",
        "prompt_eval_count": u64::MAX,
        "eval_count": 1
    })));
    let transport = transport(LocalModelProvider::Ollama, &endpoint);

    let error = transport
        .complete(&request(ToolChoice::None), &CancellationToken::new())
        .expect_err("reject overflowing usage");

    assert!(error.to_string().contains("malformed"));
}

#[test]
fn rig_ollama_stream_rejects_usage_overflow_without_panicking() {
    let records = [
        "{\"model\":\"qwen3\",\"created_at\":\"2026-08-30T00:00:00Z\",\"message\":{\"role\":\"assistant\",\"content\":\"done\"},\"done\":false}\n",
        "{\"model\":\"qwen3\",\"created_at\":\"2026-08-30T00:00:01Z\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":18446744073709551615,\"eval_count\":1}\n",
    ];
    let body = records.concat();
    let response = vec![
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes(),
        body.into_bytes(),
    ];
    let (endpoint, _) = serve_once(response);
    let transport = transport(LocalModelProvider::Ollama, &endpoint);

    let error = transport
        .stream(
            &request(ToolChoice::None),
            &CancellationToken::new(),
            &mut |_| Ok(()),
        )
        .expect_err("reject overflowing streamed usage");

    assert!(error.to_string().contains("malformed"));
}

#[test]
fn rig_transport_refuses_nested_tokio_runtime_without_panicking() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused endpoint");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    let transport = transport(LocalModelProvider::Ollama, &endpoint);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build outer runtime");

    let error = runtime.block_on(async {
        transport
            .complete(&request(ToolChoice::None), &CancellationToken::new())
            .expect_err("refuse nested runtime")
    });

    assert!(error.to_string().contains("async runtime"));
}
