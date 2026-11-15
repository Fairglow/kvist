//! Regression tests for task-run reliability defects:
//! - Retained component lock blocks subsequent runs in the same session
//! - Unmarked 500-character truncation of completion messages
//! - Non-recoverable interrupted runs with no durable diagnostic

use std::fs;
use std::process::Command;

use maerg::init::initialize;
use tempfile::TempDir;

const GENERATED_REQUIREMENTS_REVISION: &str =
    "sha256:bd53663c2dc76fdcbe58b111c0174a7550a3e3fe773a1c3e4a14196c1089dfa0";
const GENERATED_CONTRACT_REVISION: &str =
    "sha256:54b07fd8cbfb911f7e8546854b49944eb499429ea14cf302c2cba0f64238b98c";
const GENERATED_DESIGN_REVISION: &str =
    "sha256:6d6579ce1b018dce3b34e87afd72b494d27692ada8d54003b0a10ecd74abed17";

fn run_kvist(project: &TempDir, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .current_dir(project.path())
        .output()
        .expect("run kvist command")
}

fn queue() -> String {
    format!(
        r#"schema_version: 1
component:
  requirements_revision: {GENERATED_REQUIREMENTS_REVISION}
  contract_revision: {GENERATED_CONTRACT_REVISION}
  design_revision: {GENERATED_DESIGN_REVISION}
  parent_contract: null
  revalidation:
    state: current
    checked_at: 2026-08-16T12:54:50Z
    stale_since: null
    causes: []
tasks:
  - id: write-tests
    title: Write tests
    description: Define the task-command contract.
    context: Selection needs a completed test predecessor.
    purpose: Preserve lifecycle order.
    expected_outcome: The test stage is complete.
    kind: test
    status: completed
    depends_on: []
    requirements:
      - REQUIREMENTS.md#Task-selection-and-state-updates
    timestamps:
      created_at: 2026-08-16T12:54:50Z
      updated_at: 2026-08-16T12:54:50Z
      completed_at: 2026-08-16T12:54:50Z
    blocked_reason: null
  - id: implement-code
    title: Implement task command
    description: Persist one legal transition.
    context: The next task must be selected deterministically.
    purpose: Exercise durable workflow updates.
    expected_outcome: The task becomes active with audit evidence.
    kind: implementation
    status: pending
    depends_on:
      - write-tests
    requirements:
      - REQUIREMENTS.md#Task-selection-and-state-updates
    timestamps:
      created_at: 2026-08-16T12:54:50Z
      updated_at: 2026-08-16T12:54:50Z
      completed_at: null
    blocked_reason: null
"#
    )
}

fn configure_sandbox_and_track(project: &TempDir) {
    maerg::init_test_logging();
    use std::os::unix::fs::PermissionsExt;

    let runner = project.path().parent().unwrap().join("fake-galla-runner");
    fs::write(
        &runner,
        r#"#!/bin/sh
if [ "$1" = "--kvist-sandbox-probe-v1" ]; then
  runner_digest=$(sha256sum "$0" | cut -d' ' -f1)
  backend_digest=$(sha256sum /usr/bin/true | cut -d' ' -f1)
  printf '{"protocol":"kvist-sandbox-probe-v1","protocol_version":1,"runner":{"path":"%s","digest":"sha256:%s"},"backend":{"kind":"bubblewrap","path":"/usr/bin/true","digest":"sha256:%s"},"capabilities":{"namespaces":{"mount":true,"network":true,"pid":true,"ipc":true,"uts":true,"user":true},"new_session":true,"parent_death_signal":true}}\n' "$0" "$runner_digest" "$backend_digest"
  exit 0
fi
echo "fake sandbox runner"
"#,
    )
    .expect("write fake sandbox runner");
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
        .expect("make runner executable");

    let config_path = project.path().join("kvist.toml");
    let config = fs::read_to_string(&config_path).expect("read config");
    if !config.contains("[sandbox]") {
        fs::write(
            config_path,
            format!(
                "{config}\n[sandbox]\nschema_version = 1\nrunner = \"{}\"\nbackend = \"/usr/bin/true\"\nnetwork = \"deny\"\nenvironment_allowlist = []\nmount = \"component\"\n",
                runner.display()
            ),
        )
        .expect("configure sandbox");
    }

    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(project.path())
            .status()
            .expect("initialize Git")
            .success()
    );
    assert!(
        Command::new("git")
            .args(["add", "."])
            .current_dir(project.path())
            .status()
            .expect("track project artifacts")
            .success()
    );
}

/// A hermetic, loopback-only mock model gateway.
struct MockGateway {
    endpoint: String,
    address: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for MockGateway {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        let _ = std::net::TcpStream::connect(self.address.as_str());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl MockGateway {
    fn spawn(body: &str, delay: std::time::Duration) -> Self {
        let body = body.to_owned();
        Self::serve(move |_| body.clone(), delay)
    }

    fn serve(
        respond: impl Fn(&[u8]) -> String + Send + 'static,
        delay: std::time::Duration,
    ) -> Self {
        Self::serve_with_status(
            move |request| {
                let header_end = request
                    .windows(4)
                    .position(|part| part == b"\r\n\r\n")
                    .unwrap()
                    + 4;
                if request.starts_with(b"GET /models") || request.starts_with(b"GET /props?") {
                    return (404, "{}".to_string());
                }
                (200, respond(&request[header_end..]))
            },
            delay,
        )
    }

    fn serve_with_status(
        respond: impl Fn(&[u8]) -> (u16, String) + Send + 'static,
        delay: std::time::Duration,
    ) -> Self {
        Self::serve_raw(
            move |request| {
                let (status, body) = respond(request);
                format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
            },
            delay,
        )
    }

    fn serve_raw(
        respond: impl Fn(&[u8]) -> String + Send + 'static,
        delay: std::time::Duration,
    ) -> Self {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::Arc;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock gateway");
        let address = listener
            .local_addr()
            .expect("mock gateway address")
            .to_string();
        let endpoint = format!("http://{address}/v1/chat/completions");
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_loop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_loop.load(std::sync::atomic::Ordering::Acquire) {
                    break;
                }
                let Ok(mut stream) = stream else {
                    continue;
                };
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
                let mut request: Vec<u8> = Vec::new();
                let mut header_end = 0usize;
                loop {
                    let mut buffer = [0u8; 1024];
                    match stream.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            request.extend_from_slice(&buffer[..n]);
                            if let Some(index) =
                                request.windows(4).position(|chunk| chunk == b"\r\n\r\n")
                            {
                                header_end = index + 4;
                                break;
                            }
                        }
                    }
                }
                if header_end == 0 {
                    continue;
                }
                let lower = ascii_lowercase(&request[..header_end]);
                let content_length = lower
                    .windows(b"content-length:".len())
                    .position(|window| window == b"content-length:")
                    .map(|index| &lower[index + b"content-length:".len()..])
                    .and_then(|rest| {
                        let digits = rest
                            .iter()
                            .copied()
                            .skip_while(|byte| *byte == b' ' || *byte == b'\t')
                            .take_while(u8::is_ascii_digit)
                            .collect::<Vec<u8>>();
                        std::str::from_utf8(&digits).ok()?.parse().ok()
                    })
                    .unwrap_or(0);
                while request.len() < header_end + content_length {
                    let mut buffer = [0u8; 1024];
                    match stream.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => request.extend_from_slice(&buffer[..n]),
                        Err(_) => break,
                    }
                }
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
                let response = respond(&request);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        MockGateway {
            endpoint,
            address,
            stop,
            worker: Some(worker),
        }
    }
}

fn ascii_lowercase(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().map(|byte| byte.to_ascii_lowercase()).collect()
}

fn gateway_text_body(content: &str) -> String {
    serde_json::json!({
        "model": "kvist-test-mock",
        "choices": [{
            "finish_reason": "stop",
            "index": 0,
            "message": { "role": "assistant", "content": content }
        }],
    })
    .to_string()
}

fn gateway_command(endpoint: &str) -> String {
    r#"curl --request POST --json "{\"model\":\"kvist-test-mock\",\"messages\":[{\"role\":\"user\",\"content\":{prompt_json}}]}" -- "ENDPOINT_PLACEHOLDER""#
        .replace("ENDPOINT_PLACEHOLDER", endpoint)
}

/// Tests that the task lock is released after a successful run,
/// allowing a second run of the same component.
///
/// Bug: The TaskRunPrep handle is retained in the TUI after complete(),
/// keeping the lock held. The second run fails with "component is locked".
#[test]
fn lock_released_after_successful_run() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    let gateway = MockGateway::spawn(
        &gateway_text_body("Implementation complete."),
        std::time::Duration::ZERO,
    );
    let config_toml = format!(
        r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = '{}'

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "/usr/bin/echo 'mocking verify'"
"#,
        gateway_command(&gateway.endpoint)
    );
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    configure_sandbox_and_track(&project);

    // Approve test policy
    let approve_output = run_kvist(&project, &["task", "approve-policy"]);
    assert!(
        approve_output.status.success(),
        "approve failed: {}",
        String::from_utf8_lossy(&approve_output.stderr)
    );

    // First run: should succeed
    let output1 = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(
        output1.status.success(),
        "first run failed: {}",
        String::from_utf8_lossy(&output1.stderr)
    );

    // Lock should be released after successful run
    let lock_path = project.path().join("src/.kvist-task.lock");
    assert!(
        !lock_path.exists(),
        "lock file should be removed after successful run"
    );

    // Second run: should succeed (lock was released)
    let output2 = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(
        output2.status.success(),
        "second run should succeed after lock release, got: {}",
        String::from_utf8_lossy(&output2.stderr)
    );
}

/// Tests that the completion message includes the full answer,
/// not truncated at 500 characters.
///
/// Bug: TaskRunPrep::complete() truncates the answer to 500 characters
/// with no indication that truncation occurred.
#[test]
fn completion_message_includes_full_answer() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // Generate a response longer than 500 characters
    let long_answer = "A".repeat(600);
    let gateway = MockGateway::spawn(
        &gateway_text_body(&long_answer),
        std::time::Duration::ZERO,
    );
    let config_toml = format!(
        r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = '{}'

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "/usr/bin/echo 'mocking verify'"
"#,
        gateway_command(&gateway.endpoint)
    );
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    configure_sandbox_and_track(&project);

    // Approve test policy
    let approve_output = run_kvist(&project, &["task", "approve-policy"]);
    assert!(
        approve_output.status.success(),
        "approve failed: {}",
        String::from_utf8_lossy(&approve_output.stderr)
    );

    // Run the task
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The completion message should include the full 600-character answer
    assert!(
        stdout.contains(&long_answer),
        "completion message should include the full 600-character answer"
    );
}

/// Tests that an interrupted run is recoverable and produces a durable diagnostic.
///
/// Bug: When a run is interrupted (process killed), the lock remains stale,
/// the session is non-terminal, and no durable diagnostic is written.
#[test]
fn interrupted_run_is_recoverable_with_diagnostic() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // Use a gateway that delays, so we can interrupt mid-generation
    let gateway = MockGateway::spawn(
        &gateway_text_body("Implementation complete."),
        std::time::Duration::from_secs(30),
    );
    let config_toml = format!(
        r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = '{}'

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "/usr/bin/echo 'mocking verify'"
"#,
        gateway_command(&gateway.endpoint)
    );
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    configure_sandbox_and_track(&project);

    // Approve test policy
    let approve_output = run_kvist(&project, &["task", "approve-policy"]);
    assert!(
        approve_output.status.success(),
        "approve failed: {}",
        String::from_utf8_lossy(&approve_output.stderr)
    );

    // Start the task run in the background
    let mut child = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(["task", "run", ".", "implement-code"])
        .current_dir(project.path())
        .spawn()
        .expect("spawn task run");

    // Wait a moment for it to start, then kill it
    std::thread::sleep(std::time::Duration::from_secs(2));
    child.kill().expect("kill task run");
    let status = child.wait().expect("wait for killed task run");
    assert!(!status.success());

    // The lock should be cleaned up after process termination
    let lock_path = project.path().join("src/.kvist-task.lock");
    assert!(
        !lock_path.exists(),
        "lock file should be cleaned up after process termination"
    );

    // A durable diagnostic should be written somewhere
    // Check panic logs first
    let panic_log_dir = project.path().join(".kvist/panic-logs");
    let has_panic_log = panic_log_dir.exists()
        && fs::read_dir(&panic_log_dir)
            .map(|entries| entries.count() > 0)
            .unwrap_or(false);

    // Check for an unresolved attempt record
    let attempt_path = project.path().join("src/.kvist-attempts/implement-code.jsonl");
    let has_attempt = attempt_path.exists();

    assert!(
        has_panic_log || has_attempt,
        "either a panic log or an attempt journal entry should exist for interrupted run"
    );
}
