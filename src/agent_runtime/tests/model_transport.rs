use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use agent_runtime::{
    CancellationToken, DirectModelTransport, FinishReason, LocalModelProvider, ModelMessage,
    ModelRequest, ModelStreamEvent, ModelTransport, ModelTurn, ReasoningEffort, ToolChoice,
    ToolDefinition,
};
use serde_json::{Value, json};

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
                line.strip_prefix("Content-Length: ")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .expect("content length");
        while request.len() < header_end + content_length {
            let mut buffer = [0_u8; 1024];
            let count = stream.read(&mut buffer).expect("read request body");
            assert!(count > 0, "request body ended early");
            request.extend_from_slice(&buffer[..count]);
        }
        let body = serde_json::from_slice(&request[header_end..header_end + content_length])
            .expect("JSON");
        let _ = sender.send(CapturedRequest { head, body });

        for part in response_parts {
            stream.write_all(&part).expect("write fake response");
            stream.flush().expect("flush fake response");
            thread::sleep(Duration::from_millis(5));
        }
    });

    (format!("http://{address}"), receiver)
}

fn json_response(status: &str, value: Value) -> Vec<Vec<u8>> {
    let body = serde_json::to_vec(&value).expect("serialize response");
    vec![
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes(),
        body,
    ]
}

fn stream_response(content_type: &str, records: &[&str]) -> Vec<Vec<u8>> {
    let body = records.concat();
    let mut parts = vec![
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes(),
    ];
    parts.extend(records.iter().map(|record| record.as_bytes().to_vec()));
    parts
}

fn chunked_response(content_type: &str, chunks: &[&str]) -> Vec<Vec<u8>> {
    let mut parts = vec![
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
        )
        .into_bytes(),
    ];
    for chunk in chunks {
        parts.push(format!("{:x}\r\n{chunk}\r\n", chunk.len()).into_bytes());
    }
    parts.push(b"0\r\n\r\n".to_vec());
    parts
}

fn transport(provider: LocalModelProvider, endpoint: &str) -> DirectModelTransport {
    DirectModelTransport::new(provider, endpoint, Duration::from_secs(2), 1024 * 1024)
        .expect("construct transport")
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
        reasoning_effort: None,
    }
}

#[test]
fn rejects_non_loopback_and_non_http_endpoints() {
    for endpoint in [
        "https://127.0.0.1:11434",
        "http://localhost:11434",
        "http://example.com:11434",
        "http://user@127.0.0.1:11434",
        "http://127.0.0.1:11434?secret=value",
        "http://127.0.0.1:11434/api/chat",
    ] {
        let error = DirectModelTransport::new(
            LocalModelProvider::Ollama,
            endpoint,
            Duration::from_secs(1),
            1024,
        )
        .expect_err("reject unsafe endpoint");
        assert!(error.to_string().contains("endpoint"));
    }
}

#[test]
fn llama_server_unary_maps_messages_tools_and_tool_calls() {
    let (endpoint, captured) = serve_once(json_response(
        "200 OK",
        json!({
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
        }),
    ));
    let transport = transport(LocalModelProvider::LlamaServer, &endpoint);

    let turn = transport
        .complete(&request(ToolChoice::Required), &CancellationToken::new())
        .expect("complete request");

    assert_eq!(turn.text, "I will read it.");
    assert_eq!(turn.finish_reason, FinishReason::ToolCalls);
    assert_eq!(turn.response_id.as_deref(), Some("chatcmpl-1"));
    assert_eq!(turn.provider_request_id, None);
    assert_eq!(turn.model, "server-model");
    assert_eq!(turn.usage.expect("usage").total_tokens, 19);
    assert_eq!(turn.tool_intents.len(), 1);
    assert_eq!(turn.tool_intents[0].id, "call-1");
    assert_eq!(turn.tool_intents[0].name, "workspace.read");
    assert_eq!(
        turn.tool_intents[0].arguments,
        json!({"path": "REQUIREMENTS.md"})
    );

    let captured = captured.recv().expect("captured request");
    assert!(captured.head.starts_with("POST /v1/chat/completions "));
    assert_eq!(captured.body["stream"], false);
    assert_eq!(captured.body["tool_choice"], "required");
    assert_eq!(
        captured.body["tools"][0]["function"]["name"],
        "workspace.read"
    );
    assert_eq!(captured.body["messages"][2]["role"], "tool");
    assert_eq!(
        captured.body["messages"][2]["tool_call_id"],
        "previous-call"
    );
}

#[test]
fn ollama_unary_maps_native_tool_calls_and_rejects_required_choice() {
    let (endpoint, captured) = serve_once(json_response(
        "200 OK",
        json!({
            "model": "qwen3",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "function": {
                        "name": "workspace.read",
                        "arguments": {"path": "REQUIREMENTS.md"}
                    }
                }]
            },
            "done": true,
            "done_reason": "stop",
            "prompt_eval_count": 10,
            "eval_count": 4
        }),
    ));
    let ollama_transport = transport(LocalModelProvider::Ollama, &endpoint);

    let turn = ollama_transport
        .complete(&request(ToolChoice::Auto), &CancellationToken::new())
        .expect("complete request");

    assert_eq!(turn.model, "qwen3");
    assert_eq!(turn.tool_intents[0].id, "ollama-call-0");
    assert_eq!(turn.tool_intents[0].provider_id, None);
    assert_eq!(
        turn.tool_intents[0].arguments,
        json!({"path": "REQUIREMENTS.md"})
    );
    assert_eq!(turn.usage.expect("usage").total_tokens, 14);

    let captured = captured.recv().expect("captured request");
    assert!(captured.head.starts_with("POST /api/chat "));
    assert_eq!(captured.body["stream"], false);
    assert!(captured.body.get("tool_choice").is_none());
    assert_eq!(
        captured.body["messages"][2]["tool_call_id"],
        "previous-call"
    );

    let error = ollama_transport
        .complete(&request(ToolChoice::Required), &CancellationToken::new())
        .expect_err("required tool choice is unsupported by Ollama");
    assert!(error.to_string().contains("unsupported capability"));
}

#[test]
fn direct_transports_map_every_reasoning_effort_value() {
    let efforts = [
        (ReasoningEffort::None, "none"),
        (ReasoningEffort::Minimal, "minimal"),
        (ReasoningEffort::Low, "low"),
        (ReasoningEffort::Medium, "medium"),
        (ReasoningEffort::High, "high"),
        (ReasoningEffort::Xhigh, "xhigh"),
        (ReasoningEffort::Max, "max"),
    ];

    for (effort, spelling) in efforts {
        let (endpoint, captured) = serve_once(json_response(
            "200 OK",
            json!({
                "id": "chatcmpl-effort",
                "model": "test-model",
                "choices": [{
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }]
            }),
        ));
        let mut llama_request = request(ToolChoice::None);
        llama_request.reasoning_effort = Some(effort);
        transport(LocalModelProvider::LlamaServer, &endpoint)
            .complete(&llama_request, &CancellationToken::new())
            .expect("map llama-server reasoning effort");
        assert_eq!(
            captured.recv().expect("captured llama request").body["reasoning_effort"],
            spelling
        );

        let (endpoint, captured) = serve_once(json_response(
            "200 OK",
            json!({
                "model": "test-model",
                "message": {"role": "assistant", "content": "ok"},
                "done": true,
                "done_reason": "stop"
            }),
        ));
        let mut ollama_request = request(ToolChoice::None);
        ollama_request.reasoning_effort = Some(effort);
        transport(LocalModelProvider::Ollama, &endpoint)
            .complete(&ollama_request, &CancellationToken::new())
            .expect("map Ollama reasoning effort");
        let expected = if effort == ReasoningEffort::None {
            Value::Bool(false)
        } else {
            Value::String(spelling.to_owned())
        };
        assert_eq!(
            captured.recv().expect("captured Ollama request").body["think"],
            expected
        );
    }
}

#[test]
fn ollama_unary_keeps_provider_reasoning_separate_from_answer_content() {
    let (endpoint, _) = serve_once(json_response(
        "200 OK",
        json!({
            "model": "qwen3",
            "message": {
                "role": "assistant",
                "thinking": "Check the contract first.",
                "content": "The contract is satisfied."
            },
            "done": true,
            "done_reason": "stop",
            "prompt_eval_count": 3,
            "eval_count": 5
        }),
    ));

    let turn = transport(LocalModelProvider::Ollama, &endpoint)
        .complete(&request(ToolChoice::None), &CancellationToken::new())
        .expect("complete reasoning response");

    assert_eq!(turn.reasoning.as_deref(), Some("Check the contract first."));
    assert_eq!(turn.text, "The contract is satisfied.");
}

#[test]
fn llama_server_stream_assembles_text_and_fragmented_tool_arguments() {
    let records = [
        "data: {\"id\":\"chatcmpl-stream\",\"model\":\"server-model\",\"choices\":[{\"delta\":{\"content\":\"Read\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-stream\",\"model\":\"server-model\",\"choices\":[{\"delta\":{\"content\":\"ing\",\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"workspace.read\",\"arguments\":\"{\\\"path\\\":\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl-stream\",\"model\":\"server-model\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"REQUIREMENTS.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}\n\n",
        "data: [DONE]\n\n",
    ];
    let (endpoint, _) = serve_once(stream_response("text/event-stream", &records));
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
        .expect("stream request");

    assert_eq!(turn.text, "Reading");
    assert_eq!(turn.finish_reason, FinishReason::ToolCalls);
    assert_eq!(
        turn.tool_intents[0].arguments,
        json!({"path": "REQUIREMENTS.md"})
    );
    assert_eq!(
        events,
        [
            ModelStreamEvent::TextDelta("Read".to_owned()),
            ModelStreamEvent::TextDelta("ing".to_owned()),
            ModelStreamEvent::ToolIntent(turn.tool_intents[0].clone()),
        ]
    );
}

#[test]
fn ollama_stream_emits_reasoning_before_answer_text() {
    let records = [
        "{\"model\":\"qwen3\",\"message\":{\"role\":\"assistant\",\"thinking\":\"Check \",\"content\":\"\"},\"done\":false}\n",
        "{\"model\":\"qwen3\",\"message\":{\"role\":\"assistant\",\"thinking\":\"first.\",\"content\":\"Done\"},\"done\":false}\n",
        "{\"model\":\"qwen3\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":3}\n",
    ];
    let (endpoint, _) = serve_once(stream_response("application/x-ndjson", &records));
    let mut events = Vec::new();

    let turn = transport(LocalModelProvider::Ollama, &endpoint)
        .stream(
            &request(ToolChoice::None),
            &CancellationToken::new(),
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .expect("stream reasoning response");

    assert_eq!(turn.reasoning.as_deref(), Some("Check first."));
    assert_eq!(turn.text, "Done");
    assert_eq!(
        events,
        [
            ModelStreamEvent::ReasoningDelta("Check ".to_owned()),
            ModelStreamEvent::ReasoningDelta("first.".to_owned()),
            ModelStreamEvent::TextDelta("Done".to_owned()),
        ]
    );
}

#[test]
fn direct_stream_checks_deadline_after_finish_callback_returns() {
    let records = [
        "data: {\"id\":\"chatcmpl-stream\",\"model\":\"server-model\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"workspace.read\",\"arguments\":\"{\\\"path\\\":\\\"REQUIREMENTS.md\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    ];
    let (endpoint, _) = serve_once(stream_response("text/event-stream", &records));
    let transport = DirectModelTransport::new(
        LocalModelProvider::LlamaServer,
        &endpoint,
        Duration::from_millis(20),
        1024 * 1024,
    )
    .expect("construct direct transport");

    let error = transport
        .stream(
            &request(ToolChoice::Auto),
            &CancellationToken::new(),
            &mut |_| {
                thread::sleep(Duration::from_millis(50));
                Ok(())
            },
        )
        .expect_err("deadline must be observed after finish-time delivery");

    assert!(error.to_string().contains("timed out"));
}

#[test]
fn model_turn_deserializes_without_response_id() {
    let turn: ModelTurn = serde_json::from_value(json!({
        "text": "legacy",
        "tool_intents": [],
        "finish_reason": "stop",
        "provider": "ollama",
        "model": "legacy-model",
        "provider_request_id": null,
        "usage": null
    }))
    .expect("deserialize pre-response-id model turn");

    assert_eq!(turn.response_id, None);
}

#[test]
fn streaming_emits_before_the_terminal_record_arrives() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake provider");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("fake provider address")
    );
    let first = "data: {\"model\":\"server-model\",\"choices\":[{\"delta\":{\"content\":\"early\"},\"finish_reason\":null}]}\n\n";
    let last = "data: {\"model\":\"server-model\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let content_length = first.len() + last.len();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 8192];
        let _ = stream.read(&mut request).expect("read request");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n{first}"
        )
        .expect("write first record");
        stream.flush().expect("flush first record");
        thread::sleep(Duration::from_millis(300));
        stream.write_all(last.as_bytes()).expect("write terminal");
    });

    let (event_sender, event_receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        transport(LocalModelProvider::LlamaServer, &endpoint)
            .stream(
                &request(ToolChoice::None),
                &CancellationToken::new(),
                &mut |event| {
                    event_sender.send(event).expect("send stream event");
                    Ok(())
                },
            )
            .expect("stream request")
    });

    assert_eq!(
        event_receiver
            .recv_timeout(Duration::from_millis(150))
            .expect("receive event before terminal response"),
        ModelStreamEvent::TextDelta("early".to_owned())
    );
    assert_eq!(worker.join().expect("join transport").text, "early");
}

#[test]
fn decodes_chunked_stream_records_split_across_http_chunks() {
    let chunks = [
        "data: {\"model\":\"server-model\",\"choices\":[{\"delta\":{\"content\":\"ch",
        "unked\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    ];
    let (endpoint, _) = serve_once(chunked_response("text/event-stream", &chunks));
    let mut events = Vec::new();

    let turn = transport(LocalModelProvider::LlamaServer, &endpoint)
        .stream(
            &request(ToolChoice::None),
            &CancellationToken::new(),
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .expect("decode chunked stream");

    assert_eq!(turn.text, "chunked");
    assert_eq!(events, [ModelStreamEvent::TextDelta("chunked".to_owned())]);
}

#[test]
fn ollama_stream_requires_terminal_record_and_emits_complete_intent() {
    let records = [
        "{\"model\":\"qwen3\",\"message\":{\"role\":\"assistant\",\"content\":\"Use \"},\"done\":false}\n",
        "{\"model\":\"qwen3\",\"message\":{\"role\":\"assistant\",\"content\":\"the tool\",\"tool_calls\":[{\"function\":{\"name\":\"workspace.read\",\"arguments\":{\"path\":\"REQUIREMENTS.md\"}}}]},\"done\":false}\n",
        "{\"model\":\"qwen3\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":2,\"eval_count\":3}\n",
    ];
    let (endpoint, _) = serve_once(stream_response("application/x-ndjson", &records));
    let ollama_transport = transport(LocalModelProvider::Ollama, &endpoint);
    let mut events = Vec::new();

    let turn = ollama_transport
        .stream(
            &request(ToolChoice::Auto),
            &CancellationToken::new(),
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .expect("stream request");

    assert_eq!(turn.text, "Use the tool");
    assert_eq!(turn.usage.expect("usage").total_tokens, 5);
    assert!(matches!(
        events.last(),
        Some(ModelStreamEvent::ToolIntent(intent)) if intent.name == "workspace.read"
    ));

    let (endpoint, _) = serve_once(stream_response(
        "application/x-ndjson",
        &["{\"model\":\"qwen3\",\"message\":{\"content\":\"partial\"},\"done\":false}\n"],
    ));
    let error = transport(LocalModelProvider::Ollama, &endpoint)
        .stream(
            &request(ToolChoice::Auto),
            &CancellationToken::new(),
            &mut |_| Ok(()),
        )
        .expect_err("reject truncated stream");
    assert!(error.to_string().contains("terminal"));
}

#[test]
fn rejects_duplicate_calls_invalid_arguments_and_oversized_responses() {
    let duplicate_call = json!({
        "id": "chatcmpl-duplicate",
        "model": "server-model",
        "choices": [{
            "message": {
                "content": null,
                "tool_calls": [
                    {"id":"same","type":"function","function":{"name":"one","arguments":"{}"}},
                    {"id":"same","type":"function","function":{"name":"two","arguments":"{}"}}
                ]
            },
            "finish_reason": "tool_calls"
        }]
    });
    let (endpoint, _) = serve_once(json_response("200 OK", duplicate_call));
    let error = transport(LocalModelProvider::LlamaServer, &endpoint)
        .complete(&request(ToolChoice::Auto), &CancellationToken::new())
        .expect_err("reject duplicate call");
    assert!(error.to_string().contains("duplicate tool call"));

    let invalid_arguments = json!({
        "id": "chatcmpl-invalid",
        "model": "server-model",
        "choices": [{
            "message": {
                "content": null,
                "tool_calls": [{
                    "id":"call-1",
                    "type":"function",
                    "function":{"name":"workspace.read","arguments":"[]"}
                }]
            },
            "finish_reason": "tool_calls"
        }]
    });
    let (endpoint, _) = serve_once(json_response("200 OK", invalid_arguments));
    let error = transport(LocalModelProvider::LlamaServer, &endpoint)
        .complete(&request(ToolChoice::Auto), &CancellationToken::new())
        .expect_err("reject non-object call arguments");
    assert!(error.to_string().contains("tool arguments"));

    let (endpoint, _) = serve_once(json_response(
        "200 OK",
        json!({"model":"qwen3","message":{"content":"far too much"},"done":true}),
    ));
    let error = DirectModelTransport::new(
        LocalModelProvider::Ollama,
        &endpoint,
        Duration::from_secs(2),
        8,
    )
    .expect("small response limit")
    .complete(&request(ToolChoice::Auto), &CancellationToken::new())
    .expect_err("reject oversized response");
    assert!(error.to_string().contains("response limit"));
}

#[test]
fn preserves_explicit_length_finish_with_a_tool_call() {
    let (endpoint, _) = serve_once(json_response(
        "200 OK",
        json!({
            "id": "chatcmpl-length",
            "model": "server-model",
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": "partial-call",
                        "type": "function",
                        "function": {
                            "name": "workspace.read",
                            "arguments": "{\"path\":\"REQUIREMENTS.md\"}"
                        }
                    }]
                },
                "finish_reason": "length"
            }]
        }),
    ));

    let turn = transport(LocalModelProvider::LlamaServer, &endpoint)
        .complete(&request(ToolChoice::Auto), &CancellationToken::new())
        .expect("parse length-limited turn");

    assert_eq!(turn.finish_reason, FinishReason::Length);
    assert_eq!(turn.tool_intents.len(), 1);
}

#[test]
fn cancellation_and_provider_errors_are_typed_and_redacted() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = DirectModelTransport::new(
        LocalModelProvider::Ollama,
        "http://127.0.0.1:9",
        Duration::from_secs(1),
        1024,
    )
    .expect("transport")
    .complete(&request(ToolChoice::Auto), &cancellation)
    .expect_err("cancel before connection");
    assert!(error.to_string().contains("cancelled"));

    let secret = "SENTINEL_PROVIDER_SECRET";
    let body = secret.as_bytes();
    let response = vec![
        format!(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes(),
        body.to_vec(),
    ];
    let (endpoint, _) = serve_once(response);
    let error = transport(LocalModelProvider::Ollama, &endpoint)
        .complete(&request(ToolChoice::Auto), &CancellationToken::new())
        .expect_err("provider status failure");
    assert!(error.to_string().contains("status 500"));
    assert!(!error.to_string().contains(secret));
    assert!(!format!("{error:?}").contains(secret));
}

#[test]
fn deadline_interrupts_a_stalled_provider() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake provider");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("fake provider address")
    );
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 8192];
        let _ = stream.read(&mut request).expect("read request");
        thread::sleep(Duration::from_millis(500));
    });

    let error = DirectModelTransport::new(
        LocalModelProvider::Ollama,
        &endpoint,
        Duration::from_millis(150),
        1024,
    )
    .expect("transport")
    .complete(&request(ToolChoice::Auto), &CancellationToken::new())
    .expect_err("deadline must stop stalled provider");

    assert!(error.to_string().contains("timed out"));
}

#[test]
fn close_delimited_body_is_bounded_before_stream_events() {
    let record = "data: {\"model\":\"server-model\",\"choices\":[{\"delta\":{\"content\":\"must-not-emit\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{record}"
    );
    let (endpoint, _) = serve_once(vec![response.into_bytes()]);
    let mut events = Vec::new();

    let error = DirectModelTransport::new(
        LocalModelProvider::LlamaServer,
        &endpoint,
        Duration::from_secs(2),
        8,
    )
    .expect("transport")
    .stream(
        &request(ToolChoice::None),
        &CancellationToken::new(),
        &mut |event| {
            events.push(event);
            Ok(())
        },
    )
    .expect_err("reject body before emitting beyond limit");

    assert!(error.to_string().contains("response limit"));
    assert!(events.is_empty());
}
