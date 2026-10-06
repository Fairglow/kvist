use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

fn trial(finish: &str) -> std::process::Output {
    trial_text(finish, "OK", true).0
}

fn trial_text(finish: &str, content: &str, json_output: bool) -> (std::process::Output, String) {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap();
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "schema_version=1\nworking_directory={:?}\ndefault_provider='llama-server'\n\
         default_thinking_effort='none'\n[[models]]\nid='test'\n\
         provider='llama-server'\nbase_url='http://{endpoint}'\nmodel='test'\n\
         context_limit=8192\nresponse_reserve=1024\nmax_attempts=1\n",
            work.to_str().unwrap(),
        ),
    )
    .unwrap();
    let finish = finish.to_owned();
    let content = content.to_owned();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
            assert!(request.len() < 64 * 1024);
        }
        let headers = String::from_utf8(request).unwrap();
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        let request: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(request["max_tokens"], 1024);
        let delta = serde_json::json!({"model":"test","choices":[{"index":0,"delta":{"content":content},"finish_reason":null}]});
        let body = format!(
            "data: {delta}\n\n\
             data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"{finish}\"}}]}}\n\n\
             data: [DONE]\n\n"
        );
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
            Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-runner"));
    command.args([
        "--config",
        config.to_str().unwrap(),
        "--headless",
        "--log-dir",
        root.path().join("logs").to_str().unwrap(),
        "Reply OK",
    ]);
    if json_output {
        command.arg("--json");
    }
    let output = command.output().unwrap();
    server.join().unwrap();
    let transcript_path = std::fs::read_dir(root.path().join("logs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "log"))
        .unwrap();
    (output, std::fs::read_to_string(transcript_path).unwrap())
}

#[test]
fn plain_answer_escapes_terminal_commands_but_private_text_is_preserved() {
    let sentinel = "answer \u{1b}]52;c;c2VudGluZWw=\u{7}\u{9b}0m\r\nnext\tline";
    let (output, transcript) = trial_text("stop", sentinel, false);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        !text
            .chars()
            .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
    );
    assert!(text.contains("next\tline"));
    assert!(transcript.contains(sentinel));
    let (json_output, _) = trial_text("stop", sentinel, true);
    let last: Value = serde_json::from_str(
        String::from_utf8(json_output.stdout)
            .unwrap()
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(last["event"]["data"]["answer"], sentinel);
}

#[test]
fn headless_returns_ordered_versioned_events_without_a_terminal() {
    let output = trial("stop");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(!lines.is_empty());
    for (index, line) in lines.iter().enumerate() {
        assert_eq!(line["schema_version"], 1);
        assert_eq!(line["sequence"], index + 1);
    }
    let last = lines.last().unwrap();
    assert_eq!(last["event"]["type"], "run_summary");
    assert_eq!(last["event"]["data"]["disposition"], "completed");
    assert_eq!(last["event"]["data"]["answer"], "OK");
}

#[test]
fn headless_truncation_has_an_unsuccessful_final_disposition_and_exit() {
    let output = trial("length");
    assert!(!output.status.success());
    let last: Value = serde_json::from_str(
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(last["event"]["type"], "run_summary");
    assert_eq!(last["event"]["data"]["disposition"], "failed");
    assert!(last["event"]["data"]["answer"].is_null());
}

#[test]
fn headless_rejects_agent_writable_logs_before_model_io() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "schema_version=1\nworking_directory={:?}\ndefault_provider='llama-server'\n\
         default_thinking_effort='none'\n[[models]]\nid='test'\nprovider='llama-server'\n\
         base_url='http://127.0.0.1:1'\nmodel='test'\n",
            root.path().to_str().unwrap()
        ),
    )
    .unwrap();
    let logs = root.path().join("logs");
    let output = Command::new(env!("CARGO_BIN_EXE_agent-runner"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "--headless",
            "--log-dir",
            logs.to_str().unwrap(),
            "Reply OK",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside"));
    assert!(!logs.exists());
}

#[test]
fn startup_uses_selected_serving_capacity_without_a_cli_override() {
    let work = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut posted = Vec::new();
        for expected in ["GET /props?model=test ", "POST /v1/chat/completions "] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 8192];
            let end = loop {
                let count = socket.read(&mut buffer).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
                if let Some(at) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    break at + 4;
                }
            };
            let head = String::from_utf8(request[..end].to_vec()).unwrap();
            assert!(head.starts_with(expected), "{head}");
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .map(|value| value.trim().parse::<usize>().unwrap())
                .unwrap_or(0);
            while request.len() < end + length {
                let count = socket.read(&mut buffer).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let body = if expected.starts_with("GET") {
                json!({"default_generation_settings":{"n_ctx":262144}}).to_string()
            } else {
                posted = request[end..].to_vec();
                format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"choices":[{"index":0,"delta":{"content":"done"},"finish_reason":"stop"}]})
                )
            };
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
        posted
    });
    let config = work.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "schema_version=1\ndefault_provider='llama-server'\n[[models]]\nid='test'\n\
         provider='llama-server'\nbase_url='http://{endpoint}'\nmodel='test'\n"
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-runner"))
        .args(["--headless", "--json", "--config"])
        .arg(config)
        .arg("--log-dir")
        .arg(logs.path().join("private"))
        .arg("--cwd")
        .arg(work.path())
        .arg("capacity test")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request: Value = serde_json::from_slice(&server.join().unwrap()).unwrap();
    assert_eq!(request["max_tokens"], 8192);
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events[0]["event"]["data"]["context_limit"], 262144);
    assert_eq!(events[0]["event"]["data"]["context_source"], "provider");
}
