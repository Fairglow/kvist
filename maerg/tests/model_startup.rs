use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use maerg::{
    agent::{AgentExecutionRequest, execute_agent},
    config::{AgentProfile, Role, SandboxConfig, VcsSelection},
};
use serde_json::{Value, json};

fn profile(endpoint: String, context_limit: Option<usize>) -> AgentProfile {
    let mut native = sav::registry::NativeModel::new(
        sav::LocalModelProvider::LlamaServer,
        endpoint,
        "fixture".into(),
    );
    native.context_limit = context_limit;
    native.max_attempts = 1;
    AgentProfile {
        role: Role::Developer,
        profile: "fixture".into(),
        resolved_profile: Some(sav::registry::ResolvedProfile {
            name: "fixture".into(),
            profile: sav::registry::Profile::new(sav::registry::Backend::Native(native)),
            source: "providers.toml".into(),
            digest: format!("sha256:{}", "0".repeat(64)),
        }),
        command_template: String::new(),
        models: vec![],
        default_model: "fixture".into(),
        model: None,
        thinking_effort: None,
        timeout_seconds: Some(5),
        token_limit: None,
        max_output_bytes: 64 * 1024,
        redaction_values: vec![],
    }
}

fn request(listener: &TcpListener) -> (TcpStream, String, Vec<u8>) {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        assert!(Instant::now() < deadline, "expected provider request");
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(error) => panic!("accept: {error}"),
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = vec![];
        let mut chunk = [0; 8192];
        loop {
            let count = stream.read(&mut chunk).unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(split) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let header = String::from_utf8(bytes[..split].to_vec()).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= split + 4 + length {
                    return (stream, header, bytes[split + 4..].to_vec());
                }
            }
        }
        assert!(bytes.is_empty(), "incomplete provider request");
    }
}

fn reply(stream: &mut TcpStream, status: u16, body: &str) {
    write!(
        stream,
        "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

fn answer(stream: &mut TcpStream) {
    reply(
        stream,
        200,
        r#"{"model":"fixture","choices":[{"message":{"role":"assistant","content":"done"},"finish_reason":"stop"}]}"#,
    );
}

fn execute(
    directory: &Path,
    profile: &AgentProfile,
    paths: &[std::path::PathBuf],
) -> maerg::Result<maerg::agent::AgentRunResult> {
    execute_agent(
        profile,
        &SandboxConfig {
            runner: "/usr/bin/true".into(),
            backend: "/usr/bin/true".into(),
            environment_allowlist: vec![],
            acquisition: Default::default(),
        },
        None,
        None,
        AgentExecutionRequest {
            project_root: directory,
            vcs_selection: VcsSelection::Git,
            prompt: "original task",
            context_paths: paths,
            read_only_mounts: &[],
            target_dir: directory,
            task_id: "fixture",
            stream_output: false,
            role: Role::Developer,
            policy_identity: "fixture",
        },
    )
}

fn events(directory: &Path) -> Vec<Value> {
    let path = fs::read_dir(directory.join(".kvist/runs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(".events.jsonl"))
        .unwrap();
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["event"].clone())
        .collect()
}

#[test]
fn missing_unsupported_or_transient_capacity_does_not_prevent_inference() {
    for (status, body) in [
        (200, r#"{"n_ctx_train":1048576}"#),
        (404, "{}"),
        (405, "{}"),
        (501, "{}"),
        (503, "{}"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let selected = profile(format!("http://{}", listener.local_addr().unwrap()), None);
        let provider = thread::spawn(move || {
            let (mut stream, header, _) = request(&listener);
            assert!(header.starts_with("GET /models "));
            if status == 200 {
                reply(
                    &mut stream,
                    200,
                    r#"{"data":[{"id":"fixture","context_length":1048576,"meta":{"n_ctx_train":1048576}}]}"#,
                );
            } else {
                reply(&mut stream, 404, "{}");
            }
            drop(stream);
            // Standalone mode probe: /props?model=
            let (mut stream, header, _) = request(&listener);
            assert!(header.starts_with("GET /props?model=fixture "));
            reply(&mut stream, status, body);
            drop(stream);
            let (mut stream, header, _) = request(&listener);
            assert!(header.starts_with("POST /v1/chat/completions "));
            answer(&mut stream);
        });
        let result = execute(directory.path(), &selected, &[]);
        assert!(result.unwrap().success);
        provider.join().unwrap();
        let records = events(directory.path());
        assert!(
            records
                .iter()
                .any(|event| event["type"] == "capacity_unavailable")
        );
        let budget = records
            .iter()
            .find(|event| event["type"] == "model_budget")
            .unwrap();
        assert_eq!(budget["data"]["source"], "provisional");
        assert_eq!(budget["data"]["serving_capacity_known"], false);
        assert_eq!(budget["data"]["context_limit"], 32768);
    }
}

#[test]
fn quiet_capacity_probe_cannot_consume_the_whole_task_budget() {
    for wall_seconds in [5, 30] {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut selected = profile(format!("http://{}", listener.local_addr().unwrap()), None);
        selected.timeout_seconds = Some(wall_seconds);
        let provider = thread::spawn(move || {
            let (quiet, header, _) = request(&listener);
            assert!(header.starts_with("GET "));
            let started = Instant::now();
            let (mut stream, header, _) = request(&listener);
            assert!(started.elapsed() < Duration::from_secs(2));
            assert!(header.starts_with("POST "));
            answer(&mut stream);
            drop(quiet);
        });
        assert!(execute(directory.path(), &selected, &[]).unwrap().success);
        provider.join().unwrap();
        let records = events(directory.path());
        let probe = records
            .iter()
            .find(|event| event["type"] == "capacity_probe")
            .unwrap();
        let deadline = probe["data"]["deadline_ms"].as_u64().unwrap();
        assert!(deadline > 0);
        assert!(
            deadline <= 1000.min(wall_seconds * 100),
            "{deadline}ms for {wall_seconds}s task"
        );
    }
}

#[test]
fn provider_capacity_wins_over_the_configured_hint() {
    for configured in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let selected = profile(
            format!("http://{}", listener.local_addr().unwrap()),
            configured.then_some(32768),
        );
        let provider = thread::spawn(move || {
            let (mut stream, header, _) = request(&listener);
            assert!(header.starts_with("GET /models "));
            reply(
                &mut stream,
                200,
                if configured {
                    r#"{"data":[{"id":"other","meta":{"n_ctx":262144}},{"id":"fixture","status":{"value":"unloaded"},"context_length":1048576,"meta":{"n_ctx":8192,"n_ctx_train":1048576}}]}"#
                } else {
                    r#"{"data":[{"id":"other","meta":{"n_ctx":262144}},{"id":"fixture","status":{"value":"loaded"},"context_length":1048576,"meta":{"n_ctx":4096,"n_ctx_train":1048576}}]}"#
                },
            );
            drop(stream);
            if configured {
                let (mut stream, header, _) = request(&listener);
                assert!(header.starts_with("GET /props?model=fixture "));
                reply(
                    &mut stream,
                    200,
                    r#"{"default_generation_settings":{"n_ctx":4096}}"#,
                );
            }
            let (mut stream, header, body) = request(&listener);
            assert!(header.starts_with("POST "));
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["max_tokens"], 1024);
            answer(&mut stream);
        });
        assert!(execute(directory.path(), &selected, &[]).unwrap().success);
        provider.join().unwrap();
        let records = events(directory.path());
        let budget = records
            .iter()
            .find(|event| event["type"] == "model_budget")
            .unwrap();
        assert_eq!(budget["data"]["context_limit"], 4096);
        assert_eq!(budget["data"]["source"], "provider");
        assert_eq!(budget["data"]["serving_capacity_known"], true);
    }
}

#[test]
fn protected_context_overflow_continues_within_the_same_attempt() {
    protected_context_rollover(CapacityReply::Available(16384), 16384);
}

#[test]
fn protected_context_rollover_retains_confirmed_capacity_when_refresh_is_unavailable() {
    protected_context_rollover(CapacityReply::Missing, 8192);
    protected_context_rollover(CapacityReply::Transient, 8192);
}

enum CapacityReply {
    Available(usize),
    Missing,
    Transient,
}

fn protected_context_rollover(refresh: CapacityReply, expected_limit: usize) {
    let directory = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let selected = profile(
        format!("http://{}", listener.local_addr().unwrap()),
        Some(4096),
    );
    let provider = thread::spawn(move || {
        let capacity = |capacity_reply| {
            let (mut stream, header, _) = request(&listener);
            assert!(header.starts_with("GET /models "));
            match capacity_reply {
                CapacityReply::Missing => {
                    reply(&mut stream, 200, r#"{"data":[{"id":"fixture"}]}"#);
                    drop(stream);
                    let (mut stream, header, _) = request(&listener);
                    assert!(header.starts_with("GET /props?model=fixture "));
                    reply(&mut stream, 200, "{}");
                }
                CapacityReply::Available(limit) => reply(
                    &mut stream,
                    200,
                    &json!({"data":[{"id":"fixture","meta":{"n_ctx":limit}}]}).to_string(),
                ),
                CapacityReply::Transient => reply(&mut stream, 503, "{}"),
            }
        };
        capacity(CapacityReply::Available(8192));
        let (mut stream, header, _) = request(&listener);
        assert!(header.starts_with("POST "));
        reply(
            &mut stream,
            400,
            r#"{"error":{"code":400,"type":"exceed_context_size_error","message":"request (8193 tokens) exceeds the available context size (8192 tokens)","n_prompt_tokens":8193,"n_ctx":8192}}"#,
        );
        capacity(refresh);
        let (mut stream, header, body) = request(&listener);
        assert!(header.starts_with("POST "));
        let request: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(request["max_tokens"], expected_limit / 4);
        let messages = request["messages"].to_string();
        assert!(messages.contains("original task"));
        assert!(messages.contains("Predecessor: protected run"));
        answer(&mut stream);
    });
    assert!(execute(directory.path(), &selected, &[]).unwrap().success);
    provider.join().unwrap();
    let records = events(directory.path());
    let rollover = records
        .iter()
        .find(|event| event["type"] == "context_rollover")
        .unwrap();
    assert_eq!(rollover["data"]["previous_generation"], 1);
    assert_eq!(rollover["data"]["context_limit"], expected_limit);
    assert_eq!(rollover["data"]["source"], "provider");
    assert_eq!(rollover["data"]["proposals_replayed"], false);
}

#[test]
fn invalid_capacity_and_authorization_failures_do_not_become_fallbacks() {
    for standalone in [false, true] {
        for (status, body) in [
            (
                200,
                if standalone {
                    r#"{"default_generation_settings":{"n_ctx":0}}"#
                } else {
                    r#"{"data":[{"id":"fixture","meta":{"n_ctx":0},"context_length":1048576}]}"#
                },
            ),
            (200, "not JSON"),
            (401, "{}"),
            (403, "{}"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let selected = profile(format!("http://{}", listener.local_addr().unwrap()), None);
            let provider = thread::spawn(move || {
                if standalone {
                    let (mut stream, header, _) = request(&listener);
                    assert!(header.starts_with("GET /models "));
                    reply(&mut stream, 404, "{}");
                }
                let (mut stream, header, _) = request(&listener);
                assert!(header.starts_with(if standalone {
                    "GET /props?model=fixture "
                } else {
                    "GET /models "
                }));
                reply(&mut stream, status, body);
            });
            assert!(execute(directory.path(), &selected, &[]).is_err());
            provider.join().unwrap();
            assert!(
                !events(directory.path())
                    .iter()
                    .any(|event| event["type"] == "model_request")
            );
        }
    }
}

#[test]
fn provisional_budget_preserves_large_approved_input_without_claiming_capacity() {
    let directory = tempfile::tempdir().unwrap();
    let contents = "approved intent stays exact\n".repeat(6000);
    fs::write(directory.path().join("REQUIREMENTS.md"), &contents).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let selected = profile(format!("http://{}", listener.local_addr().unwrap()), None);
    let provider = thread::spawn(move || {
        // Router mode probe: /models returns 404 to simulate standalone mode
        let (mut stream, _, _) = request(&listener);
        reply(&mut stream, 404, "{}");
        drop(stream);
        // Standalone mode probe: /props?model=
        let (mut stream, _, _) = request(&listener);
        reply(&mut stream, 200, "{}");
        drop(stream);
        let (mut stream, _, bytes) = request(&listener);
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains(&json!(contents).to_string())
        );
        answer(&mut stream);
    });
    assert!(
        execute(
            directory.path(),
            &selected,
            &["/workspace/component/REQUIREMENTS.md".into()]
        )
        .unwrap()
        .success
    );
    provider.join().unwrap();
    let records = events(directory.path());
    let budget = records
        .iter()
        .find(|event| event["type"] == "model_budget")
        .unwrap();
    assert!(budget["data"]["context_limit"].as_u64().unwrap() > 32768);
    assert_eq!(budget["data"]["serving_capacity_known"], false);
    let request = records
        .iter()
        .find(|event| event["type"] == "model_request")
        .unwrap();
    assert!(request["data"]["estimated_input_tokens"].as_u64().unwrap() > 8192);
}

#[test]
fn unknown_capacity_does_not_hide_provider_context_rejection() {
    let directory = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let selected = profile(format!("http://{}", listener.local_addr().unwrap()), None);
    let provider = thread::spawn(move || {
        // Router mode probe: /models returns 404 to simulate standalone mode
        let (mut stream, header, _) = request(&listener);
        assert!(header.starts_with("GET /models "));
        reply(&mut stream, 404, "{}");
        drop(stream);
        // Standalone mode probe: /props?model=
        let (mut stream, header, _) = request(&listener);
        assert!(header.starts_with("GET /props?model=fixture "));
        reply(&mut stream, 200, "{}");
        drop(stream);
        let (mut stream, header, _) = request(&listener);
        assert!(header.starts_with("POST "));
        reply(&mut stream, 400, r#"{"error":"context window exceeded"}"#);
    });
    let result = execute(directory.path(), &selected, &[]).unwrap();
    assert!(!result.success);
    assert!(result.stderr.contains("HTTP status 400"));
    provider.join().unwrap();
    assert_eq!(
        events(directory.path())
            .iter()
            .filter(|event| event["type"] == "model_request")
            .count(),
        1,
        "provider context rejection must not be retried or treated as success"
    );
}
