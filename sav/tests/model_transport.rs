use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use sav::{
    CancellationToken, DirectModelTransport, Error, FinishReason, LocalModelProvider, ModelMessage,
    ModelRequest, ModelStreamEvent, ModelTransport, ModelTurn, ReasoningEffort, ToolChoice,
    ToolDefinition,
};
use serde_json::{Value, json};

struct CapturedRequest {
    head: String,
    body: Value,
}

fn capture_request(stream: &mut TcpStream) -> CapturedRequest {
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
    let body =
        serde_json::from_slice(&request[header_end..header_end + content_length]).expect("JSON");
    CapturedRequest { head, body }
}

fn serve_once(response_parts: Vec<Vec<u8>>) -> (String, mpsc::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake provider");
    let address = listener.local_addr().expect("fake provider address");
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let _ = sender.send(capture_request(&mut stream));

        for part in response_parts {
            stream.write_all(&part).expect("write fake response");
            stream.flush().expect("flush fake response");
            thread::sleep(Duration::from_millis(5));
        }
    });

    (format!("http://{address}"), receiver)
}

#[derive(Clone, Copy)]
enum FixtureFraming {
    Length,
    Chunked,
    OneChunk,
    Close,
}

fn serve_timed(
    framing: FixtureFraming,
    records: Vec<(Duration, String)>,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind timed provider");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("provider address")
    );
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept timed request");
        let _ = capture_request(&mut stream);
        let length: usize = records.iter().map(|(_, record)| record.len()).sum();
        let framing_head = match framing {
            FixtureFraming::Length => format!("Content-Length: {length}\r\n"),
            FixtureFraming::Chunked | FixtureFraming::OneChunk => {
                "Transfer-Encoding: chunked\r\n".to_owned()
            }
            FixtureFraming::Close => String::new(),
        };
        let mut head = format!("HTTP/1.1 200 OK\r\n{framing_head}Connection: close\r\n\r\n");
        if matches!(framing, FixtureFraming::OneChunk) {
            head.push_str(&format!("{length:x}\r\n"));
        }
        if stream.write_all(head.as_bytes()).is_err() {
            return;
        }
        for (delay, record) in records {
            thread::sleep(delay);
            let part = if matches!(framing, FixtureFraming::Chunked) {
                format!("{:x}\r\n{record}\r\n", record.len())
            } else {
                record
            };
            if stream.write_all(part.as_bytes()).is_err() {
                return;
            }
        }
        if matches!(framing, FixtureFraming::OneChunk) {
            let _ = stream.write_all(b"\r\n");
        }
        if matches!(framing, FixtureFraming::Chunked | FixtureFraming::OneChunk) {
            let _ = stream.write_all(b"0\r\n\r\n");
        }
    });
    (endpoint, worker)
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
        output_schema: None,
        max_output_tokens: None,
    }
}

fn request_with_output_bound(bound: Option<u32>) -> ModelRequest {
    let mut value = serde_json::to_value(request(ToolChoice::None)).expect("serialize request");
    if let Some(bound) = bound {
        value["max_output_tokens"] = json!(bound);
    }
    serde_json::from_value(value).expect("deserialize bounded request")
}

#[test]
fn reliability_large_context_requests_are_not_limited_by_the_old_two_mib_ceiling() {
    let (endpoint, captured) = serve_once(text_response(LocalModelProvider::LlamaServer, false));
    let transport = DirectModelTransport::new(
        LocalModelProvider::LlamaServer,
        &endpoint,
        Duration::from_secs(5),
        1024 * 1024,
    )
    .unwrap();
    let mut large = request(ToolChoice::None);
    large.messages = vec![ModelMessage::User("x".repeat(3 * 1024 * 1024))];
    assert!(
        transport
            .complete(&large, &CancellationToken::new())
            .is_ok()
    );
    assert!(
        captured.recv_timeout(Duration::from_secs(1)).unwrap().body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .len()
            > 2 * 1024 * 1024
    );
    large.messages = vec![ModelMessage::User("x".repeat(8 * 1024 * 1024))];
    assert!(matches!(
        transport.complete(&large, &CancellationToken::new()),
        Err(Error::InvalidModelRequest { .. })
    ));
}

fn text_response(provider: LocalModelProvider, streaming: bool) -> Vec<Vec<u8>> {
    match (provider, streaming) {
        (LocalModelProvider::Ollama, false) => json_response(
            "200 OK",
            json!({"message": {"content": "ok"}, "done": true, "done_reason": "stop"}),
        ),
        (LocalModelProvider::Ollama, true) => stream_response(
            "application/x-ndjson",
            &["{\"message\":{\"content\":\"ok\"},\"done\":true,\"done_reason\":\"stop\"}\n"],
        ),
        (LocalModelProvider::LlamaServer, false) => json_response(
            "200 OK",
            json!({"choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}]}),
        ),
        (LocalModelProvider::LlamaServer, true) => stream_response(
            "text/event-stream",
            &[
                "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
        ),
    }
}

fn finish_response(
    provider: LocalModelProvider,
    streaming: bool,
    reason: Option<Value>,
    has_tools: bool,
) -> Vec<Vec<u8>> {
    let mut message = json!({"content": "answer"});
    if has_tools {
        let arguments = match provider {
            LocalModelProvider::LlamaServer => json!("{}"),
            LocalModelProvider::Ollama => json!({}),
        };
        message["tool_calls"] = json!([{
            "index": 0,
            "id": "call-1",
            "type": "function",
            "function": {"name": "workspace.read", "arguments": arguments}
        }]);
    }
    match (provider, streaming) {
        (LocalModelProvider::LlamaServer, false) => {
            let mut choice = json!({"message": message});
            if let Some(reason) = reason {
                choice["finish_reason"] = reason;
            }
            json_response("200 OK", json!({"choices": [choice]}))
        }
        (LocalModelProvider::LlamaServer, true) => {
            let mut final_choice = json!({"delta": {}});
            if let Some(reason) = reason {
                final_choice["finish_reason"] = reason;
            }
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                json!({"choices": [{"delta": message, "finish_reason": null}]}),
                json!({"choices": [final_choice]})
            );
            stream_response("text/event-stream", &[&body])
        }
        (LocalModelProvider::Ollama, false) => {
            let mut root = json!({"message": message, "done": true});
            if let Some(reason) = reason {
                root["done_reason"] = reason;
            }
            json_response("200 OK", root)
        }
        (LocalModelProvider::Ollama, true) => {
            let mut terminal = json!({"message": {}, "done": true});
            if let Some(reason) = reason {
                terminal["done_reason"] = reason;
            }
            let body = format!(
                "{}\n{}\n",
                json!({"message": message, "done": false}),
                terminal
            );
            stream_response("application/x-ndjson", &[&body])
        }
    }
}

fn assert_llama_finish(streaming: bool, reason: Option<Value>, expected: FinishReason) {
    for has_tools in [false, true] {
        let (endpoint, _) = serve_once(finish_response(
            LocalModelProvider::LlamaServer,
            streaming,
            reason.clone(),
            has_tools,
        ));
        let transport = transport(LocalModelProvider::LlamaServer, &endpoint);
        let cancellation = CancellationToken::new();
        let turn = if streaming {
            transport.stream(&request(ToolChoice::Auto), &cancellation, &mut |_| Ok(()))
        } else {
            transport.complete(&request(ToolChoice::Auto), &cancellation)
        }
        .expect("retain finish reason and tool intent for caller decision");
        assert_eq!(turn.finish_reason, expected);
        assert_eq!(turn.tool_intents.len(), usize::from(has_tools));
    }
}

#[test]
fn llama_server_unary_explicit_stop_with_tools_remains_stop() {
    assert_llama_finish(false, Some(json!("stop")), FinishReason::Stop);
}

#[test]
fn llama_server_stream_explicit_stop_with_tools_remains_stop() {
    assert_llama_finish(true, Some(json!("stop")), FinishReason::Stop);
}

#[test]
fn llama_server_unary_missing_or_null_finish_is_unknown_even_with_tools() {
    for reason in [None, Some(Value::Null)] {
        assert_llama_finish(false, reason, FinishReason::Other("unknown".to_owned()));
    }
}

#[test]
fn llama_server_stream_missing_or_null_finish_is_unknown_even_with_tools() {
    for reason in [None, Some(Value::Null)] {
        assert_llama_finish(true, reason, FinishReason::Other("unknown".to_owned()));
    }
}

#[test]
fn llama_server_stream_terminal_marker_alone_does_not_infer_stop() {
    let (endpoint, _) = serve_once(stream_response("text/event-stream", &["data: [DONE]\n\n"]));
    let turn = transport(LocalModelProvider::LlamaServer, &endpoint)
        .stream(
            &request(ToolChoice::None),
            &CancellationToken::new(),
            &mut |_| Ok(()),
        )
        .expect("retain unknown finish on an empty terminal stream");
    assert_eq!(
        turn.finish_reason,
        FinishReason::Other("unknown".to_owned())
    );
}

#[test]
fn llama_server_stream_null_or_absent_deltas_do_not_erase_explicit_finish() {
    let records = [
        "data: {\"choices\":[{\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"length\"}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{}}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    ];
    let (endpoint, _) = serve_once(stream_response("text/event-stream", &records));
    let turn = transport(LocalModelProvider::LlamaServer, &endpoint)
        .stream(
            &request(ToolChoice::None),
            &CancellationToken::new(),
            &mut |_| Ok(()),
        )
        .expect("retain explicit finish across metadata deltas");
    assert_eq!(turn.finish_reason, FinishReason::Length);
    assert_eq!(turn.usage.expect("terminal usage").total_tokens, 3);
}

#[test]
fn both_providers_reject_malformed_finish_reasons() {
    for provider in [LocalModelProvider::LlamaServer, LocalModelProvider::Ollama] {
        for streaming in [false, true] {
            for reason in [
                json!(false),
                json!(7),
                json!({}),
                json!([]),
                json!("stop\n"),
                json!("x".repeat(129)),
            ] {
                let (endpoint, _) = serve_once(vec![
                    finish_response(provider, streaming, Some(reason), true).concat(),
                ]);
                let transport = transport(provider, &endpoint);
                let cancellation = CancellationToken::new();
                let mut events = Vec::new();
                let error = if streaming {
                    transport.stream(&request(ToolChoice::Auto), &cancellation, &mut |event| {
                        events.push(event);
                        Ok(())
                    })
                } else {
                    transport.complete(&request(ToolChoice::Auto), &cancellation)
                }
                .expect_err("reject malformed finish reason");
                assert!(matches!(error, Error::MalformedModelResponse { .. }));
                assert!(
                    events
                        .iter()
                        .all(|event| !matches!(event, ModelStreamEvent::ToolIntent(_)))
                );
            }
        }
    }
}

#[test]
fn ollama_native_terminal_stop_or_absence_keeps_tool_call_convention() {
    for streaming in [false, true] {
        for reason in [None, Some(Value::Null), Some(json!("stop"))] {
            for has_tools in [false, true] {
                let (endpoint, _) = serve_once(finish_response(
                    LocalModelProvider::Ollama,
                    streaming,
                    reason.clone(),
                    has_tools,
                ));
                let transport = transport(LocalModelProvider::Ollama, &endpoint);
                let cancellation = CancellationToken::new();
                let turn = if streaming {
                    transport.stream(&request(ToolChoice::Auto), &cancellation, &mut |_| Ok(()))
                } else {
                    transport.complete(&request(ToolChoice::Auto), &cancellation)
                }
                .expect("decode native Ollama completion convention");
                let expected = if has_tools {
                    FinishReason::ToolCalls
                } else {
                    FinishReason::Stop
                };
                assert_eq!(turn.finish_reason, expected);
                assert_eq!(turn.tool_intents.len(), usize::from(has_tools));
            }
        }
    }
}

#[test]
fn model_request_output_bound_has_optional_canonical_wire_shape() {
    let unbounded = request_with_output_bound(None);
    let value = serde_json::to_value(&unbounded).expect("serialize unbounded request");
    assert!(value.get("max_output_tokens").is_none());
    for bound in [1, 128, 1_048_576] {
        let bounded = request_with_output_bound(Some(bound));
        assert_eq!(
            serde_json::to_value(&bounded).expect("serialize bounded request")["max_output_tokens"],
            bound
        );
    }
    let mut value = serde_json::to_value(unbounded).expect("serialize request");
    value["max_output_tokens"] = Value::Null;
    let decoded: ModelRequest = serde_json::from_value(value).expect("decode null output bound");
    assert!(
        serde_json::to_value(decoded)
            .expect("serialize absent output bound")
            .get("max_output_tokens")
            .is_none()
    );
}

#[test]
fn direct_transports_encode_output_bounds_and_preserve_absent_defaults() {
    for provider in [LocalModelProvider::LlamaServer, LocalModelProvider::Ollama] {
        for streaming in [false, true] {
            for bound in [None, Some(1), Some(128), Some(1_048_576)] {
                let (endpoint, captured) = serve_once(text_response(provider, streaming));
                let transport = transport(provider, &endpoint);
                let request = request_with_output_bound(bound);
                let cancellation = CancellationToken::new();
                let turn = if streaming {
                    transport.stream(&request, &cancellation, &mut |_| Ok(()))
                } else {
                    transport.complete(&request, &cancellation)
                }
                .expect("complete output-bound fixture");
                assert_eq!(turn.text, "ok");
                let body = captured.recv().expect("captured request").body;
                assert_eq!(body["stream"], streaming);
                match (provider, bound) {
                    (LocalModelProvider::LlamaServer, Some(bound)) => {
                        assert_eq!(body["max_tokens"], bound);
                        assert!(body.get("options").is_none());
                    }
                    (LocalModelProvider::Ollama, Some(bound)) => {
                        assert_eq!(body["options"], json!({"num_predict": bound}));
                        assert!(body.get("max_tokens").is_none());
                    }
                    (_, None) => {
                        assert!(body.get("max_tokens").is_none());
                        assert!(body.get("options").is_none());
                    }
                }
                assert!(body.get("max_output_tokens").is_none());
            }
        }
    }
}

#[test]
fn invalid_output_bounds_fail_before_connect_for_both_providers() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused endpoint");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    for provider in [LocalModelProvider::LlamaServer, LocalModelProvider::Ollama] {
        let transport =
            DirectModelTransport::new(provider, &endpoint, Duration::from_millis(100), 1024)
                .expect("construct transport");
        for bound in [0, 1_048_577, u32::MAX] {
            for streaming in [false, true] {
                let request = request_with_output_bound(Some(bound));
                let cancellation = CancellationToken::new();
                let error = if streaming {
                    transport.stream(&request, &cancellation, &mut |_| Ok(()))
                } else {
                    transport.complete(&request, &cancellation)
                }
                .expect_err("reject invalid bound");
                assert!(
                    matches!(error, Error::InvalidModelRequest { .. }),
                    "unexpected error: {error:?}"
                );
                assert!(error.to_string().contains("output token"));
                assert!(matches!(
                    listener.accept(),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
                ));
            }
        }
    }
}

#[test]
fn direct_transports_encode_provider_native_output_schemas() {
    let schema = json!({
        "type": "object",
        "properties": {
            "result": {"type": "string"},
            "optional_note": {"type": "string"}
        },
        "required": ["result"],
        "additionalProperties": false
    });

    let (endpoint, captured) = serve_once(json_response(
        "200 OK",
        json!({
            "model": "test-model",
            "message": {"role": "assistant", "content": "{\"result\":\"ok\"}"},
            "done": true,
            "done_reason": "stop"
        }),
    ));
    let mut ollama_request = request(ToolChoice::None);
    ollama_request.output_schema = Some(schema.clone());
    transport(LocalModelProvider::Ollama, &endpoint)
        .complete(&ollama_request, &CancellationToken::new())
        .expect("send Ollama output schema");
    assert_eq!(
        captured.recv().expect("Ollama request").body["format"],
        schema
    );

    let (endpoint, captured) = serve_once(json_response(
        "200 OK",
        json!({
            "id": "chatcmpl-schema",
            "model": "test-model",
            "choices": [{
                "message": {"role": "assistant", "content": "{\"result\":\"ok\"}"},
                "finish_reason": "stop"
            }]
        }),
    ));
    let mut llama_request = request(ToolChoice::None);
    llama_request.output_schema = Some(schema.clone());
    transport(LocalModelProvider::LlamaServer, &endpoint)
        .complete(&llama_request, &CancellationToken::new())
        .expect("send llama-server output schema");
    let body = captured.recv().expect("llama-server request").body;
    assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
    assert_eq!(body["response_format"]["json_schema"]["strict"], true);
    assert!(
        body["grammar"]
            .as_str()
            .expect("grammar present in request")
            .contains("root ::=")
    );
}

#[test]
fn output_schema_rejects_non_objects_and_callable_tools_before_io() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused endpoint");
    listener
        .set_nonblocking(true)
        .expect("make unused endpoint nonblocking");
    let endpoint = format!("http://{}", listener.local_addr().expect("local address"));
    let transport = transport(LocalModelProvider::LlamaServer, &endpoint);

    let mut invalid = request(ToolChoice::None);
    invalid.output_schema = Some(json!(true));
    assert!(
        transport
            .complete(&invalid, &CancellationToken::new())
            .expect_err("reject non-object schema")
            .to_string()
            .contains("output schema")
    );
    invalid.output_schema = Some(json!({"type": 7}));
    assert!(
        transport
            .complete(&invalid, &CancellationToken::new())
            .expect_err("reject malformed schema keyword")
            .to_string()
            .contains("`type`")
    );
    invalid.output_schema = Some(json!({
        "oneOf": [{"type": "string"}, {"type": "number"}]
    }));
    assert!(
        transport
            .complete(&invalid, &CancellationToken::new())
            .expect_err("reject unsupported schema keyword")
            .to_string()
            .contains("oneOf")
    );

    let mut incompatible = request(ToolChoice::Auto);
    incompatible.output_schema = Some(json!({"type": "object"}));
    assert!(
        transport
            .complete(&incompatible, &CancellationToken::new())
            .expect_err("reject schema and callable tools")
            .to_string()
            .contains("cannot be combined")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
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
        "data: {\"id\":\"chatcmpl-stream\",\"model\":\"server-model\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Check first.\"},\"finish_reason\":null}]}\n\n",
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
    assert_eq!(turn.reasoning.as_deref(), Some("Check first."));
    assert_eq!(turn.finish_reason, FinishReason::ToolCalls);
    assert_eq!(
        turn.tool_intents[0].arguments,
        json!({"path": "REQUIREMENTS.md"})
    );
    assert_eq!(
        events,
        [
            ModelStreamEvent::ReasoningDelta("Check first.".to_owned()),
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
fn both_providers_preserve_non_success_finishes_with_tool_intents() {
    for provider in [LocalModelProvider::LlamaServer, LocalModelProvider::Ollama] {
        for streaming in [false, true] {
            for (reason, expected) in [
                ("length", FinishReason::Length),
                ("content_filter", FinishReason::ContentFilter),
                (
                    "provider_stopped",
                    FinishReason::Other("provider_stopped".to_owned()),
                ),
            ] {
                let calls = match provider {
                    LocalModelProvider::LlamaServer => json!([{
                        "index": 0,
                        "id": "call-1",
                        "type": "function",
                        "function": {"name": "workspace.read", "arguments": "{\"path\":\"file\"}"}
                    }]),
                    LocalModelProvider::Ollama => json!([{
                        "id": "call-1",
                        "function": {"name": "workspace.read", "arguments": {"path": "file"}}
                    }]),
                };
                let message = json!({"content": "partial", "tool_calls": calls});
                let response = match (provider, streaming) {
                    (LocalModelProvider::LlamaServer, false) => json_response(
                        "200 OK",
                        json!({"choices": [{"message": message, "finish_reason": reason}]}),
                    ),
                    (LocalModelProvider::LlamaServer, true) => {
                        let record = format!(
                            "data: {}\n\ndata: [DONE]\n\n",
                            json!({"choices": [{"delta": message, "finish_reason": reason}]})
                        );
                        stream_response("text/event-stream", &[&record])
                    }
                    (LocalModelProvider::Ollama, false) => json_response(
                        "200 OK",
                        json!({"message": message, "done": true, "done_reason": reason}),
                    ),
                    (LocalModelProvider::Ollama, true) => {
                        let record = format!(
                            "{}\n",
                            json!({"message": message, "done": true, "done_reason": reason})
                        );
                        stream_response("application/x-ndjson", &[&record])
                    }
                };
                let (endpoint, _) = serve_once(response);
                let transport = transport(provider, &endpoint);
                let cancellation = CancellationToken::new();
                let mut events = Vec::new();
                let turn = if streaming {
                    transport.stream(&request(ToolChoice::Auto), &cancellation, &mut |event| {
                        events.push(event);
                        Ok(())
                    })
                } else {
                    transport.complete(&request(ToolChoice::Auto), &cancellation)
                }
                .expect("decode explicitly terminated turn");
                assert_eq!(turn.finish_reason, expected);
                assert_eq!(turn.tool_intents.len(), 1);
                if streaming {
                    assert_eq!(
                        events,
                        [
                            ModelStreamEvent::TextDelta("partial".to_owned()),
                            ModelStreamEvent::ToolIntent(turn.tool_intents[0].clone())
                        ]
                    );
                }
            }
        }
    }
}

#[test]
fn llama_server_stream_rejects_truncated_sse_and_provider_errors() {
    for records in [
        vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n",
        ],
        vec!["data: {\"choices\":["],
        vec!["data: {\"error\":{\"message\":\"SENTINEL_PROVIDER_SECRET\"}}\n\n"],
    ] {
        let (endpoint, _) = serve_once(stream_response("text/event-stream", &records));
        let error = transport(LocalModelProvider::LlamaServer, &endpoint)
            .stream(
                &request(ToolChoice::None),
                &CancellationToken::new(),
                &mut |_| Ok(()),
            )
            .expect_err("reject incomplete or failed provider stream");
        assert!(matches!(error, Error::MalformedModelResponse { .. }));
        assert!(!error.to_string().contains("SENTINEL_PROVIDER_SECRET"));
        assert!(!format!("{error:?}").contains("SENTINEL_PROVIDER_SECRET"));
    }
}

#[test]
fn streaming_cancellation_after_a_delta_returns_no_terminal_turn() {
    for provider in [LocalModelProvider::LlamaServer, LocalModelProvider::Ollama] {
        let (endpoint, _) = serve_once(vec![text_response(provider, true).concat()]);
        let cancellation = CancellationToken::new();
        let mut events = Vec::new();
        let error = transport(provider, &endpoint)
            .stream(&request(ToolChoice::None), &cancellation, &mut |event| {
                events.push(event);
                cancellation.cancel();
                Ok(())
            })
            .expect_err("reject cancellation during streaming delivery");
        assert!(matches!(error, Error::ModelTransportCancelled));
        assert_eq!(events, [ModelStreamEvent::TextDelta("ok".to_owned())]);
    }
}

#[test]
fn streams_reject_duplicate_call_ids_before_delivering_tool_intents() {
    for provider in [LocalModelProvider::LlamaServer, LocalModelProvider::Ollama] {
        let records = match provider {
            LocalModelProvider::LlamaServer => vec![
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"same\",\"function\":{\"name\":\"one\",\"arguments\":\"{}\"}},{\"index\":1,\"id\":\"same\",\"function\":{\"name\":\"two\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n",
            ],
            LocalModelProvider::Ollama => vec![
                "{\"message\":{\"tool_calls\":[{\"id\":\"same\",\"function\":{\"name\":\"one\",\"arguments\":{}}}]},\"done\":false}\n",
                "{\"message\":{\"tool_calls\":[{\"id\":\"same\",\"function\":{\"name\":\"two\",\"arguments\":{}}}]},\"done\":true,\"done_reason\":\"stop\"}\n",
            ],
        };
        let content_type = match provider {
            LocalModelProvider::LlamaServer => "text/event-stream",
            LocalModelProvider::Ollama => "application/x-ndjson",
        };
        let (endpoint, _) = serve_once(stream_response(content_type, &records));
        let mut events = Vec::new();
        let error = transport(provider, &endpoint)
            .stream(
                &request(ToolChoice::Auto),
                &CancellationToken::new(),
                &mut |event| {
                    events.push(event);
                    Ok(())
                },
            )
            .expect_err("duplicate identities fail closed");
        assert!(matches!(error, Error::DuplicateToolCall));
        assert!(events.is_empty());
    }
}

#[test]
fn llama_server_stream_rejects_malformed_tool_deltas_without_repairing_them() {
    for malformed_function in [
        json!(false),
        json!({"name": 7}),
        json!({"arguments": {"path": "file"}}),
    ] {
        let first = format!(
            "data: {}\n\n",
            json!({"choices": [{"delta": {"tool_calls": [{
                "index": 0, "id": "call-1", "function": malformed_function
            }]}, "finish_reason": null}]})
        );
        let records = [
            first.as_str(),
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"workspace.read\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ];
        let (endpoint, _) = serve_once(stream_response("text/event-stream", &records));
        let mut events = Vec::new();
        let error = transport(LocalModelProvider::LlamaServer, &endpoint)
            .stream(
                &request(ToolChoice::Auto),
                &CancellationToken::new(),
                &mut |event| {
                    events.push(event);
                    Ok(())
                },
            )
            .expect_err("malformed deltas cannot be silently discarded");
        assert!(matches!(error, Error::MalformedModelResponse { .. }));
        assert!(events.is_empty());
    }
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
fn stream_with_deadline_applies_the_override_not_the_base() {
    // A caller-supplied deadline overrides the transport's configured one, so a
    // retrying loop can grant an attempt more time without changing the
    // transport's deadline. The configured deadline and watchdogs are set long
    // here so only the short override can stop the stalled turn, proving the
    // override is honored rather than the configured deadline.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake provider");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("fake provider address")
    );
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set read timeout");
        let mut request = [0_u8; 8192];
        let _ = stream.read(&mut request).expect("read request");
        // Hang without responding; only the override deadline can end the turn.
        thread::sleep(Duration::from_secs(30));
    });

    let transport = DirectModelTransport::new(
        LocalModelProvider::Ollama,
        &endpoint,
        Duration::from_secs(30),
        1024,
    )
    .expect("transport")
    .with_slot_timeout(Duration::from_secs(60))
    .with_ttft_timeout(Duration::from_secs(60));

    let start = Instant::now();
    let error = transport
        .stream_with_deadline(
            &request(ToolChoice::Auto),
            &CancellationToken::new(),
            &mut |_| Ok(()),
            Duration::from_millis(150),
        )
        .expect_err("the override deadline aborts the stalled turn");
    assert!(
        error.to_string().contains("timed out"),
        "unexpected error: {error:?}"
    );
    // It must stop near the 150ms override, not after the 30s configured deadline.
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "the override deadline was not honored"
    );
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

#[test]
fn slot_allocation_timeout_aborts_stalled_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake provider");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("fake provider address")
    );
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 8192];
        let _ = stream.read(&mut request).expect("read request");
        // Server hangs without sending headers (e.g. stalled slot allocation / VRAM prefill)
        thread::sleep(Duration::from_secs(2));
    });

    let transport = DirectModelTransport::new(
        LocalModelProvider::Ollama,
        &endpoint,
        Duration::from_secs(10),
        1024,
    )
    .expect("transport")
    .with_slot_timeout(Duration::from_millis(150));

    let start = Instant::now();
    let error = transport
        .complete(&request(ToolChoice::Auto), &CancellationToken::new())
        .expect_err("slot allocation timeout must stop stalled server");

    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(2), "elapsed: {:?}", elapsed);
    assert!(
        error.to_string().contains("slot allocation timed out"),
        "error: {error}"
    );
}

#[test]
fn ttft_watchdog_aborts_stalled_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake provider");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("fake provider address")
    );
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 8192];
        let _ = stream.read(&mut request).expect("read request");
        // Server sends HTTP headers immediately
        let headers = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        stream.write_all(headers).expect("write headers");
        // But then hangs before emitting the first token chunk (e.g. prefill decode stall)
        thread::sleep(Duration::from_secs(2));
    });

    let transport = DirectModelTransport::new(
        LocalModelProvider::LlamaServer,
        &endpoint,
        Duration::from_secs(10),
        4096,
    )
    .expect("transport")
    .with_ttft_timeout(Duration::from_millis(150));

    let mut events = Vec::new();
    let start = Instant::now();
    let error = transport
        .stream(
            &request(ToolChoice::None),
            &CancellationToken::new(),
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .expect_err("TTFT watchdog must stop stream before first token");

    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(2), "elapsed: {:?}", elapsed);
    assert!(
        error
            .to_string()
            .contains("time-to-first-token watchdog timed out"),
        "error: {error}"
    );
    assert!(events.is_empty());
}

#[test]
fn inter_token_cadence_watchdog_aborts_hung_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake provider");
    let endpoint = format!(
        "http://{}",
        listener.local_addr().expect("fake provider address")
    );
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 8192];
        let _ = stream.read(&mut request).expect("read request");
        // Server sends HTTP headers
        let headers = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
        stream.write_all(headers).expect("write headers");
        // Server emits first token chunk immediately
        let chunk1_data = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n";
        let chunk1_header = format!("{:x}\r\n", chunk1_data.len());
        stream
            .write_all(chunk1_header.as_bytes())
            .expect("write chunk header");
        stream.write_all(chunk1_data).expect("write chunk data");
        stream.write_all(b"\r\n").expect("write chunk crlf");
        // But then hangs before emitting the next token (e.g. GPU deadlocked midway)
        thread::sleep(Duration::from_secs(2));
    });

    let transport = DirectModelTransport::new(
        LocalModelProvider::LlamaServer,
        &endpoint,
        Duration::from_secs(10),
        4096,
    )
    .expect("transport")
    .with_cadence_timeout(Duration::from_millis(150));

    let mut events = Vec::new();
    let start = Instant::now();
    let error = transport
        .stream(
            &request(ToolChoice::None),
            &CancellationToken::new(),
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .expect_err("Cadence watchdog must abort hung stream between tokens");

    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(2), "elapsed: {:?}", elapsed);
    assert!(
        error
            .to_string()
            .contains("inter-token cadence watchdog timed out"),
        "error: {error}"
    );
    assert_eq!(events.len(), 1);
}

fn cadence_fixture(provider: LocalModelProvider, first_kind: &str) -> Vec<(Duration, String)> {
    let message = match first_kind {
        "reasoning" => json!({"reasoning_content": "thinking", "thinking": "thinking"}),
        "tool" => match provider {
            LocalModelProvider::LlamaServer => json!({"tool_calls": [{
                "index": 0, "id": "call-1",
                "function": {"name": "workspace.read", "arguments": "{"}
            }]}),
            LocalModelProvider::Ollama => json!({"tool_calls": [{
                "id": "call-1", "function": {"name": "workspace.read", "arguments": {}}
            }]}),
        },
        _ => json!({"content": "Hello"}),
    };
    let first = match provider {
        LocalModelProvider::LlamaServer => {
            format!("data: {}\n\n", json!({"choices": [{"delta": message}]}))
        }
        LocalModelProvider::Ollama => {
            format!("{}\n", json!({"message": message, "done": false}))
        }
    };
    let mut records = vec![(Duration::ZERO, first)];
    for index in 0..18 {
        let noise = match (provider, index % 4) {
            (LocalModelProvider::LlamaServer, 0) => ": heartbeat\n\n",
            (_, 1) => "\n",
            (LocalModelProvider::LlamaServer, 2) => {
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\n"
            }
            (LocalModelProvider::LlamaServer, _) => {
                "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\",\"reasoning_content\":\"\",\"tool_calls\":[]}}]}\n\n"
            }
            (LocalModelProvider::Ollama, _) => {
                "{\"message\":{\"content\":\"\",\"thinking\":\"\"},\"done\":false,\"eval_count\":1}\n"
            }
        };
        records.push((Duration::from_millis(30), noise.to_owned()));
    }
    let last = match provider {
        LocalModelProvider::LlamaServer if first_kind == "tool" => {
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n"
        }
        LocalModelProvider::LlamaServer => {
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
        }
        LocalModelProvider::Ollama => "{\"message\":{},\"done\":true,\"done_reason\":\"stop\"}\n",
    };
    records.push((Duration::ZERO, last.to_owned()));
    records
}

fn assert_semantic_cadence(
    provider: LocalModelProvider,
    framing: FixtureFraming,
    first_kind: &str,
) {
    let (endpoint, worker) = serve_timed(framing, cadence_fixture(provider, first_kind));
    let error = transport(provider, &endpoint)
        .with_cadence_timeout(Duration::from_millis(120))
        .stream(
            &request(ToolChoice::Auto),
            &CancellationToken::new(),
            &mut |_| Ok(()),
        )
        .expect_err("metadata cannot extend decoded generation cadence");
    worker.join().expect("join heartbeat provider");
    assert!(
        matches!(error, Error::InterTokenCadenceTimedOut { .. }),
        "{error:?}"
    );
}

#[test]
fn cadence_ignores_heartbeats_empty_control_and_usage_frames_in_every_framing() {
    for provider in [LocalModelProvider::LlamaServer, LocalModelProvider::Ollama] {
        for framing in [
            FixtureFraming::Length,
            FixtureFraming::Chunked,
            FixtureFraming::OneChunk,
            FixtureFraming::Close,
        ] {
            assert_semantic_cadence(provider, framing, "text");
        }
    }
}

#[test]
fn reasoning_and_native_tool_progress_start_cadence_before_intent_delivery() {
    for provider in [LocalModelProvider::LlamaServer, LocalModelProvider::Ollama] {
        for first_kind in ["reasoning", "tool"] {
            assert_semantic_cadence(provider, FixtureFraming::Chunked, first_kind);
        }
    }
}

#[test]
fn cadence_does_not_start_on_slow_fragmented_initial_records() {
    for provider in [LocalModelProvider::LlamaServer, LocalModelProvider::Ollama] {
        for framing in [
            FixtureFraming::Length,
            FixtureFraming::Chunked,
            FixtureFraming::OneChunk,
            FixtureFraming::Close,
        ] {
            let mut records = cadence_fixture(provider, "text");
            let first = records.remove(0).1;
            let last = records.pop().expect("terminal record").1;
            let split = first.len() / 3;
            let (endpoint, worker) = serve_timed(
                framing,
                vec![
                    (Duration::ZERO, first[..split].to_owned()),
                    (
                        Duration::from_millis(150),
                        first[split..2 * split].to_owned(),
                    ),
                    (Duration::from_millis(150), first[2 * split..].to_owned()),
                    (Duration::ZERO, last),
                ],
            );
            let turn = transport(provider, &endpoint)
                .with_cadence_timeout(Duration::from_millis(80))
                .with_ttft_timeout(Duration::from_secs(1))
                .stream(
                    &request(ToolChoice::None),
                    &CancellationToken::new(),
                    &mut |_| Ok(()),
                )
                .expect("body fragments before decoded progress do not arm cadence");
            worker.join().expect("join fragmented provider");
            assert_eq!(turn.text, "Hello");
        }
    }
}

#[test]
fn cadence_tracks_tool_fragments_and_reasoning_before_late_tool_events() {
    for provider in [LocalModelProvider::LlamaServer, LocalModelProvider::Ollama] {
        for framing in [
            FixtureFraming::Length,
            FixtureFraming::Chunked,
            FixtureFraming::OneChunk,
            FixtureFraming::Close,
        ] {
            let mut records = vec![(
                Duration::ZERO,
                cadence_fixture(provider, "text").remove(0).1,
            )];
            let reasoning = match provider {
                LocalModelProvider::LlamaServer => {
                    "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"working\"}}]}\n\n"
                }
                LocalModelProvider::Ollama => {
                    "{\"message\":{\"thinking\":\"working\"},\"done\":false}\n"
                }
            };
            records.push((Duration::from_millis(60), reasoning.to_owned()));
            match provider {
                LocalModelProvider::LlamaServer => {
                    for function in [
                        json!({"name": "workspace.", "arguments": "{"}),
                        json!({"name": "read"}),
                        json!({"arguments": "\"path\":"}),
                        json!({"arguments": "\"file\"}"}),
                    ] {
                        records.push((
                            Duration::from_millis(60),
                            format!(
                                "data: {}\n\n",
                                json!({"choices": [{"delta": {"tool_calls": [{
                                    "index": 0, "id": "call-1", "function": function
                                }]}}]})
                            ),
                        ));
                    }
                    records.push((Duration::ZERO,
                        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n".to_owned()));
                }
                LocalModelProvider::Ollama => {
                    for index in 0..4 {
                        records.push((Duration::from_millis(60), format!(
                            "{}\n",
                            json!({"message": {"tool_calls": [{
                                "id": format!("call-{index}"),
                                "function": {"name": "workspace.read", "arguments": {"path": "file"}}
                            }]}, "done": false})
                        )));
                    }
                    records.push((
                        Duration::ZERO,
                        "{\"message\":{},\"done\":true,\"done_reason\":\"stop\"}\n".to_owned(),
                    ));
                }
            }
            let (endpoint, worker) = serve_timed(framing, records);
            let mut events = Vec::new();
            let turn = transport(provider, &endpoint)
                .with_cadence_timeout(Duration::from_millis(130))
                .stream(
                    &request(ToolChoice::Auto),
                    &CancellationToken::new(),
                    &mut |event| {
                        events.push(event);
                        Ok(())
                    },
                )
                .expect("generation fragments refresh cadence before tool events");
            worker.join().expect("join tool-fragment provider");
            assert_eq!(turn.finish_reason, FinishReason::ToolCalls);
            assert_eq!(turn.reasoning.as_deref(), Some("working"));
            assert!(matches!(&events[2], ModelStreamEvent::ToolIntent(_)));
            assert_eq!(
                turn.tool_intents.len(),
                if provider == LocalModelProvider::Ollama {
                    4
                } else {
                    1
                }
            );
        }
    }
}

#[test]
fn caller_deadline_and_cancellation_still_bound_heartbeat_streams() {
    for cancel in [false, true] {
        let provider = LocalModelProvider::LlamaServer;
        let (endpoint, worker) =
            serve_timed(FixtureFraming::Chunked, cadence_fixture(provider, "text"));
        let cancellation = CancellationToken::new();
        let (start_sender, start_receiver) = mpsc::channel();
        let cancel_token = cancellation.clone();
        let canceller = thread::spawn(move || {
            start_receiver.recv().expect("first decoded event");
            if cancel {
                thread::sleep(Duration::from_millis(30));
                cancel_token.cancel();
            }
        });
        let mut start_sender = Some(start_sender);
        let error = transport(provider, &endpoint)
            .with_cadence_timeout(Duration::from_secs(1))
            .stream_with_deadline(
                &request(ToolChoice::None),
                &cancellation,
                &mut |_| {
                    if let Some(sender) = start_sender.take() {
                        sender.send(()).expect("notify cancellation worker");
                    }
                    Ok(())
                },
                Duration::from_millis(150),
            )
            .expect_err("caller authority still interrupts heartbeat streams");
        worker.join().expect("join heartbeat provider");
        canceller.join().expect("join cancellation worker");
        assert!(
            if cancel {
                matches!(error, Error::ModelTransportCancelled)
            } else {
                matches!(error, Error::ModelTransportTimedOut)
            },
            "{error:?}"
        );
    }
}
