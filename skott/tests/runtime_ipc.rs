use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use clap::Parser;
use sav::observation::ControlRequest;
use sav::{
    CancellationToken, ModelRequest, ModelStreamEvent, ModelTransport, ModelTurn, ToolIntent,
};
use skott::{
    AgentSession, ContextManager, Event, Recorder, RetryPolicy, RunLimits, RunSummary,
    ToolExecutor, ToolOutcome,
};

#[test]
fn control_opt_in_requires_logged_execution_not_other_cli_modes() {
    assert!(skott::Cli::try_parse_from(["skott", "--allow-control", "--headless", "task"]).is_ok());
    for conflict in [
        "--no-logs",
        "--allow-host-execution",
        "--setup",
        "--list-models",
    ] {
        assert!(
            skott::Cli::try_parse_from(["skott", "--allow-control", conflict]).is_err(),
            "{conflict} must not expose execution controls"
        );
    }
    assert!(
        skott::Cli::try_parse_from(["skott", "--allow-control", "--attach", "run.jsonl"]).is_err()
    );
}

struct Provider {
    started: mpsc::Sender<()>,
    requests: Arc<Mutex<Vec<ModelRequest>>>,
}
impl ModelTransport for Provider {
    fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
        unreachable!()
    }
    fn stream(
        &self,
        request: &ModelRequest,
        token: &CancellationToken,
        _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
    ) -> sav::Result<ModelTurn> {
        let first = {
            let mut requests = self.requests.lock().unwrap();
            requests.push(request.clone());
            requests.len() == 1
        };
        if first {
            self.started.send(()).unwrap();
            while !token.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            return Err(sav::Error::ModelTransportCancelled);
        }
        assert!(
            !token.is_cancelled(),
            "an idle cancellation must not poison a later prompt"
        );
        Ok(ModelTurn {
            text: "done".into(),
            reasoning: None,
            tool_intents: vec![],
            finish_reason: sav::FinishReason::Stop,
            provider: sav::LocalModelProvider::LlamaServer,
            model: "test".into(),
            response_id: None,
            provider_request_id: None,
            usage: None,
        })
    }
    fn deadline(&self) -> Duration {
        Duration::from_secs(5)
    }
}
struct Executor;
impl ToolExecutor for Executor {
    fn execute(&self, _: &ToolIntent, _: &CancellationToken) -> skott::Result<ToolOutcome> {
        panic!("no tools")
    }
}
struct Record;
impl Recorder for Record {
    fn session_start(&mut self) -> skott::Result<()> {
        Ok(())
    }
    fn request(&mut self, _: &ModelRequest, _: u32) -> skott::Result<()> {
        Ok(())
    }
    fn turn_start(&mut self, _: &ModelTurn) -> skott::Result<usize> {
        Ok(0)
    }
    fn turn_finish(&mut self, _: usize, _: &Option<sav::ModelUsage>, _: &str) -> skott::Result<()> {
        Ok(())
    }
    fn tool_dispatch(&mut self, _: usize, _: &ToolIntent) -> skott::Result<()> {
        Ok(())
    }
    fn tool_result(
        &mut self,
        _: usize,
        _: &str,
        _: &str,
        _: &serde_json::Value,
        _: &ToolOutcome,
    ) -> skott::Result<()> {
        Ok(())
    }
    fn session_finish(&mut self, _: &RunSummary) -> skott::Result<()> {
        Ok(())
    }
}
fn session() -> AgentSession {
    let model = skott::Model {
        id: "test".into(),
        provider: skott::ModelProvider::LlamaServer,
        base_url: "http://127.0.0.1:1".into(),
        model: "test".into(),
        is_default: false,
        context_limit: Some(8192),
        response_reserve: Some(1024),
        deadline_secs: 5,
        max_attempts: 3,
        retry_base_delay_secs: 1,
        retry_max_delay_secs: 1,
        cadence_timeout_secs: 0,
    };
    AgentSession::new(
        model,
        sav::ReasoningEffort::None,
        vec![],
        "host instructions".into(),
    )
}
fn send_control(
    path: &Path,
    request: &ControlRequest,
) -> sav::Result<sav::observation::ControlReply> {
    sav::observation::send_control(path, request, Duration::from_secs(1))
}
#[test]
fn owned_worker_socket_steers_quiet_provider_rejects_idle_and_disappears_on_shutdown() {
    let (started, ready) = mpsc::channel();
    let requests = Arc::new(Mutex::new(vec![]));
    let (handle, events, prompts) = skott::start_session(
        session(),
        Provider {
            started,
            requests: requests.clone(),
        },
        Executor,
        ContextManager::new(8192, 4),
        Some(Box::new(Record)),
        RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(1)),
        5,
        RunLimits::default(),
    )
    .unwrap();
    let path = handle.enable_controls().unwrap();
    assert_eq!(handle.enable_controls().unwrap(), path);
    assert!(path.exists());
    assert_eq!(
        send_control(&path, &ControlRequest::guide(1, "idle guidance"))
            .unwrap()
            .status,
        "rejected"
    );
    assert_eq!(
        send_control(&path, &ControlRequest::cancel(2))
            .unwrap()
            .status,
        "rejected"
    );
    prompts.send("original task".into()).unwrap();
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let reply = send_control(&path, &ControlRequest::guide(3, "additional guidance")).unwrap();
    assert_eq!(reply.status, "queued");
    assert!(reply.message.unwrap().starts_with("guidance #"));
    loop {
        match events.recv_timeout(Duration::from_secs(2)).unwrap() {
            Event::Finished { .. } => break,
            Event::Failed(message) => panic!("{message}"),
            _ => {}
        }
    }
    let observed = requests.lock().unwrap();
    assert_eq!(observed.len(), 2);
    let second = serde_json::to_string(&observed[1]).unwrap();
    assert!(second.contains("original task") && second.contains("additional guidance"));
    assert!(
        !second.contains(path.to_str().unwrap()),
        "capability must never enter model context"
    );
    drop(observed);
    assert_eq!(
        send_control(&path, &ControlRequest::guide(4, "late guidance"))
            .unwrap()
            .status,
        "rejected"
    );
    assert_eq!(handle.controls().next_task().unwrap(), None);
    handle.cancel();
    assert_eq!(
        prompts.send("explicit later task".into()).unwrap().kind,
        skott::SubmissionKind::Task
    );
    loop {
        match events.recv_timeout(Duration::from_secs(2)).unwrap() {
            Event::Finished { .. } => break,
            Event::Failed(message) => panic!("{message}"),
            _ => {}
        }
    }
    assert_eq!(requests.lock().unwrap().len(), 3);
    handle.shutdown();
    assert!(!path.exists(), "shutdown must remove the owned socket");
}
#[test]
fn worker_refuses_control_socket_without_required_recording() {
    let (started, _ready) = mpsc::channel();
    let (handle, _events, _prompts) = skott::start_session(
        session(),
        Provider {
            started,
            requests: Arc::new(Mutex::new(vec![])),
        },
        Executor,
        ContextManager::new(8192, 4),
        None,
        RetryPolicy::default(),
        5,
        RunLimits::default(),
    )
    .unwrap();
    assert!(handle.enable_controls().is_err());
}

#[test]
fn binary_refuses_control_combination_before_reading_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_skott"))
        .args(["--allow-control", "--no-logs", "--config", "missing.toml"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used"));
    assert!(!Path::new("missing.toml").exists());
}

#[test]
fn natural_worker_exit_removes_its_control_socket_without_dropping_handle() {
    let (started, _ready) = mpsc::channel();
    let (handle, _events, prompts) = skott::start_session(
        session(),
        Provider {
            started,
            requests: Arc::new(Mutex::new(vec![])),
        },
        Executor,
        ContextManager::new(8192, 4),
        Some(Box::new(Record)),
        RetryPolicy::default(),
        5,
        RunLimits::default(),
    )
    .unwrap();
    let path = handle.enable_controls().unwrap();
    drop(prompts);
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while handle.is_running() {
        assert!(
            std::time::Instant::now() < deadline,
            "worker did not close its control listener"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!path.exists());
    assert!(handle.enable_controls().is_err());
}

#[test]
fn headless_refuses_an_agent_writable_control_socket_location() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "schema_version=1\nworking_directory={:?}\ndefault_provider='llama-server'\n\
         [tool_profiles]\nrust='off'\npython='off'\njavascript='off'\ngo='off'\nc='off'\n\
         [[models]]\nid='test'\nprovider='llama-server'\nbase_url='http://127.0.0.1:1'\n\
         model='test'\ncontext_limit=8192\nresponse_reserve=1024\n",
            work.to_str().unwrap()
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_skott"))
        .arg("--config")
        .arg(config)
        .args([
            "--headless",
            "--allow-control",
            "--model",
            "test",
            "--log-dir",
        ])
        .arg(root.path().join("logs"))
        .arg("must not run")
        .env("TMPDIR", &work)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostics.contains("outside"), "{diagnostics}");
    assert!(!diagnostics.contains("private control socket:"));
    assert_eq!(std::fs::read_dir(work).unwrap().count(), 0);
}

#[test]
fn headless_binary_opt_in_steers_quiet_generation_without_disclosing_capability() {
    use serde_json::{Value, json};
    use std::io::{BufRead, Read, Write};
    use std::net::TcpListener;
    use std::process::Stdio;
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let logs = root.path().join("logs");
    std::fs::create_dir(&work).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap();
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        format!(
            "schema_version=1\nworking_directory={:?}\ndefault_provider='llama-server'\n\
         [tool_profiles]\nrust='off'\npython='off'\njavascript='off'\ngo='off'\nc='off'\n\
         [[models]]\nid='test'\nprovider='llama-server'\nbase_url='http://{endpoint}'\n\
         model='test'\ncontext_limit=8192\nresponse_reserve=1024\nmax_attempts=3\n",
            work.to_str().unwrap()
        ),
    )
    .unwrap();
    let (requests, received) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let mut generation = 0;
        loop {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut headers = Vec::new();
            let mut byte = [0];
            while !headers.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                headers.push(byte[0]);
                assert!(headers.len() < 65536);
            }

            let headers = String::from_utf8(headers).unwrap();
            let size = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let mut body = vec![0; size];
            socket.read_exact(&mut body).unwrap();
            if headers.starts_with("GET ") {
                let body = json!({"model":"test"}).to_string();
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
                continue;
            }
            generation += 1;
            requests
                .send(serde_json::from_slice::<Value>(&body).unwrap())
                .unwrap();
            if generation == 1 {
                write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 10000\r\n\r\n").unwrap();
                socket.flush().unwrap();
                assert_eq!(
                    socket.read(&mut byte).unwrap(),
                    0,
                    "steering must close quiet generation"
                );
            } else {
                let body = format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"choices":[{"index":0,"delta":{"content":"guided answer"},"finish_reason":"stop"}]})
                );
                write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                break;
            }
        }
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_skott"))
        .arg("--config")
        .arg(config)
        .args([
            "--headless",
            "--json",
            "--allow-control",
            "--model",
            "test",
            "--log-dir",
        ])
        .arg(&logs)
        .arg("original binary task")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (paths, path_rx) = mpsc::channel();
    let stderr_reader = std::thread::spawn(move || {
        let mut captured = String::new();
        for line in std::io::BufReader::new(stderr).lines() {
            let line = line.unwrap();
            if let Some(path) = line.strip_prefix("private control socket: ") {
                paths.send(std::path::PathBuf::from(path)).unwrap();
            }
            captured.push_str(&line);
            captured.push('\n');
        }
        captured
    });
    let path = path_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let first = received.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(first.to_string().contains("original binary task"));
    let reply = send_control(
        &path,
        &ControlRequest::guide(51, "supplementary binary guidance"),
    )
    .unwrap();
    assert_eq!(reply.status, "queued");
    let second = received.recv_timeout(Duration::from_secs(5)).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("headless controlled run did not finish");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let output = child.wait_with_output().unwrap();
    let diagnostics = stderr_reader.join().unwrap();
    assert!(output.status.success(), "{diagnostics}");
    server.join().unwrap();
    let capability = path.to_str().unwrap();
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(output.contains("guided answer"));
    assert!(!output.contains(capability));
    assert!(second.to_string().contains("supplementary binary guidance"));
    assert!(second.to_string().contains("original binary task"));
    assert!(!second.to_string().contains(capability));
    assert!(!path.exists());
    let mut directories = vec![logs];
    while let Some(directory) = directories.pop() {
        for file in std::fs::read_dir(directory).unwrap() {
            let file = file.unwrap();
            if file.file_type().unwrap().is_dir() {
                directories.push(file.path());
            } else {
                let bytes = std::fs::read(file.path()).unwrap();
                assert!(
                    !bytes
                        .windows(capability.len())
                        .any(|part| part == capability.as_bytes()),
                    "operator capability must never enter durable records"
                );
            }
        }
    }
}
