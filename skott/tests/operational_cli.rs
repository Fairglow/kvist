use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use serde_json::{Value, json};

fn trial(
    repetitive: bool,
    flags: &[&str],
    json_output: bool,
) -> (std::process::Output, Vec<String>, usize) {
    trial_with_read_validation(repetitive, flags, json_output, false)
}

fn trial_with_read_validation(
    repetitive: bool,
    flags: &[&str],
    json_output: bool,
    invalid_read: bool,
) -> (std::process::Output, Vec<String>, usize) {
    let root = tempfile::tempdir_in(".").unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let work = root_path.join("work");
    std::fs::create_dir(&work).unwrap();
    let runner = root_path.join("runner");
    std::fs::hard_link(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/subprocess_runner.sh"),
        &runner,
    )
    .unwrap();
    std::fs::write(root_path.join("script"), if invalid_read {
        "cat >/dev/null; printf complete-invalid-native-page; printf completed-diagnostic >&2; exit 0\n"
    } else {"cat >/dev/null; printf 'permission fixture \\033]52;c;unsafe\\007' >&2; exit 7\n"}).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = listener.local_addr().unwrap();
    let config = root_path.join("skott.toml");
    std::fs::write(
        &config,
        format!(
            "schema_version=1\nworking_directory={work:?}\ndefault_provider='llama-server'\n\
        [sandbox]\nrunner={runner:?}\nbackend='/usr/bin/true'\n\
        [tool_profiles]\nrust='off'\npython='off'\njavascript='off'\ngo='off'\nc='off'\n\
        [[models]]\nid='test'\nprovider='llama-server'\nbase_url='http://{endpoint}'\n\
        model='test'\ncontext_limit=32768\nresponse_reserve=1024\nmax_attempts=3\n"
        ),
    )
    .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let server = std::thread::spawn(move || {
        while !stopping.load(Ordering::SeqCst) {
            let (mut socket, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut headers = Vec::new();
            let mut byte = [0];
            while !headers.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                headers.push(byte[0]);
            }
            let headers = String::from_utf8(headers).unwrap();
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            let mut body = vec![0; length];
            socket.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["max_tokens"], 1024);
            let index = called.fetch_add(1, Ordering::SeqCst);
            let delta = if repetitive {
                json!({"content":"intentional repeated output ".repeat(150)})
            } else if index == 0 {
                json!({"tool_calls":[{"index":0,"id":"failed-call","type":"function",
                    "function":{"name":if invalid_read {"read_file"} else {"shell"},
                        "arguments":if invalid_read {"{\"path\":\"/workspace/a\"}"} else {"{\"command\":\"true\"}"}}}]})
            } else {
                json!({"content":"failure observed"})
            };
            let finish = if !repetitive && index == 0 {
                "tool_calls"
            } else {
                "stop"
            };
            let first =
                json!({"model":"test","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
            let body = format!(
                "data: {first}\n\ndata: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"{finish}\"}}]}}\n\ndata: [DONE]\n\n"
            );
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    let logs = root_path.join("logs");
    let mut command = Command::new(env!("CARGO_BIN_EXE_skott"));
    command
        .arg("--config")
        .arg(config)
        .arg("--headless")
        .arg("--log-dir")
        .arg(&logs)
        .arg("perform one bounded operation")
        .args(flags)
        .env("HOME", &root_path)
        .env("XDG_CONFIG_HOME", root_path.join("user-config"));
    if json_output {
        command.arg("--json");
    }
    let output = command.output().unwrap();
    stop.store(true, Ordering::SeqCst);
    server.join().unwrap();
    let records = if logs.exists() {
        fn collect(path: &std::path::Path, text: &mut Vec<String>) {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    collect(&path, text);
                } else {
                    text.push(std::fs::read_to_string(path).unwrap());
                }
            }
        }
        let mut records = Vec::new();
        collect(&logs, &mut records);
        records
    } else {
        vec![]
    };
    (output, records, calls.load(Ordering::SeqCst))
}

#[test]
fn phase4_headless_validation_error_preserves_actual_completed_totals_and_outcome() {
    let (output, records, calls) = trial_with_read_validation(false, &[], true, true);
    assert!(!output.status.success());
    assert_eq!(calls, 1, "observation validation failure must not replay");
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let summary = &events.last().unwrap()["event"]["data"];
    assert_eq!(summary["turns"], 1, "{summary}");
    assert_eq!(summary["tools_executed"], 1, "{summary}");
    assert_eq!(summary["stop_reason"]["kind"], "failed");
    assert!(
        records
            .iter()
            .any(|text| text.contains("complete-invalid-native-page")
                && text.contains("completed-diagnostic"))
    );
}

#[test]
fn phase4_plain_headless_failure_exposes_bounded_cause_without_terminal_controls() {
    let (output, records, calls) = trial(false, &[], false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(calls, 2);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for fragment in [
        "permission fixture",
        "process_exit",
        "may_have_effects",
        "inspect",
    ] {
        assert!(stderr.contains(fragment), "{stderr}");
    }
    assert!(!stderr.contains('\u{1b}'));
    assert!(!stderr.contains('\u{7}'));
    assert!(
        records
            .iter()
            .any(|text| text.contains("tool_failure") && text.contains("process_exit"))
    );
    assert!(
        records
            .iter()
            .any(|text| text.contains("may_have_effects") && text.contains("permission fixture"))
    );
}

#[test]
fn phase4_headless_typed_failure_and_stop_are_durable() {
    let (output, records, calls) = trial(false, &[], true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(calls, 2);
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let failure = events
        .iter()
        .find(|event| event["event"]["type"] == "tool_failure")
        .expect("typed operator diagnostic");
    let failure = &failure["event"]["data"]["failure"];
    assert_eq!(failure["category"], "process_exit");
    assert_eq!(failure["effects"], "may_have_effects");
    assert!(
        failure["message"]
            .as_str()
            .unwrap()
            .contains("permission fixture")
    );
    assert_eq!(
        events.last().unwrap()["event"]["data"]["stop_reason"]["kind"],
        "completed"
    );
    assert!(
        records
            .iter()
            .any(|text| text.contains("\"stop_reason\":{\"kind\":\"completed\"}"))
    );
}

#[test]
fn phase4_native_cli_generation_exception_is_explicit_and_default_is_on() {
    let (output, _, calls) = trial(true, &[], true);
    assert!(!output.status.success());
    assert_eq!(calls, 3, "two finite corrections, then honest stall");
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        events.last().unwrap()["event"]["data"]["stop_reason"]["kind"],
        "stalled"
    );
    let (output, _, calls) = trial(true, &["--disable-generation-loop-detection"], true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(calls, 1);
    let (output, _, _) = trial(
        false,
        &[
            "--disable-generation-loop-detection",
            "--max-run-tokens",
            "1",
        ],
        true,
    );
    assert!(
        !output.status.success(),
        "the exception must not remove explicit budgets"
    );
    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        events.last().unwrap()["event"]["data"]["stop_reason"],
        json!({"kind":"limit","resource":"model_tokens"})
    );
}
