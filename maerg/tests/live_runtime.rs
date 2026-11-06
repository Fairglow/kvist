use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use kvist::agent::{AgentExecutionRequest, execute_agent};
use kvist::config::{AgentProfile, Model, Role, SandboxConfig, VcsSelection};
use sav::observation::JournalFollower;

#[path = "live_runtime/generation.rs"]
mod generation;
#[path = "live_runtime/progress.rs"]
mod progress;

fn profile(endpoint: &str) -> AgentProfile {
    let command = format!("curl --json '{{\"model\":\"fixture\"}}' '{endpoint}/api/chat'");
    AgentProfile {
        role: Role::Developer,
        profile: "fixture".into(),
        resolved_profile: None,
        command_template: command.clone(),
        models: vec![Model {
            name: "fixture".into(),
            command,
            system_prompt: None,
        }],
        default_model: "fixture".into(),
        model: None,
        thinking_effort: None,
        timeout_seconds: 5,
        token_limit: None,
        max_output_bytes: 64 * 1024,
        redaction_values: vec!["private-value".into()],
    }
}

fn invoke(workdir: &Path, profile: &AgentProfile) {
    let result = execute_agent(
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
            project_root: workdir,
            vcs_selection: VcsSelection::Git,
            prompt: "fixture task",
            context_paths: &[],
            read_only_mounts: &[],
            target_dir: workdir,
            task_id: "live-task",
            stream_output: false,
            role: Role::Developer,
            policy_identity: "sha256:fixture",
        },
    )
    .expect("execute");
    assert!(result.success);
}

#[test]
fn protected_run_is_observable_before_provider_completion_and_is_redacted() {
    let dir = tempfile::tempdir().expect("workspace");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let provider = thread::spawn(move || {
        loop {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("deadline");
            // Read the full request (head plus the Content-Length body) before
            // treating the request as received. The client writes the head and
            // the body in separate writes, so a single read can return only the
            // head; blocking on the release gate while the body is still in
            // flight would close the connection mid-write and time out the turn.
            let mut bytes = Vec::new();
            let mut chunk = [0_u8; 8192];
            loop {
                let count = stream.read(&mut chunk).expect("read");
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(split) = bytes.windows(4).position(|tail| tail == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..split]).to_ascii_lowercase();
                    let length = header
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:").map(str::trim))
                        .expect("length")
                        .parse::<usize>()
                        .expect("number");
                    if bytes.len() >= split + 4 + length {
                        break;
                    }
                }
            }
            if bytes.is_empty() {
                continue;
            }

            started_tx.send(()).expect("started");
            release_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("release");
            let body = serde_json::json!({
                "model":"private-value","message":{"role":"assistant","content":"done private-value"},
                "done":true,"done_reason":"stop"
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("response");
            break;
        }
    });
    let workdir = dir.path().to_owned();
    let worker = thread::spawn(move || {
        let mut selection = profile("not-a-gateway");
        selection.resolved_profile = Some(sav::registry::ResolvedProfile {
            name: "shared-fixture".into(),
            profile: sav::registry::Profile::new(sav::registry::Backend::Native({
                let mut native = sav::registry::NativeModel::new(
                    sav::LocalModelProvider::Ollama,
                    endpoint,
                    "fixture".into(),
                );
                native.context_limit = Some(4096);
                native
            })),
            source: workdir.join("providers.toml"),
            digest: format!("sha256:{}", "0".repeat(64)),
        });
        selection.models.clear();
        invoke(&workdir, &selection);
    });
    started_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("inference started");
    let journal = fs::read_dir(dir.path().join(".kvist/runs"))
        .expect("runs")
        .map(|entry| entry.expect("entry").path())
        .find(|path| path.to_string_lossy().ends_with(".events.jsonl"))
        .expect("live journal exists before inference completes");
    let mut follower = JournalFollower::open(&journal).expect("follower");
    let initial = follower.poll().expect("live records");
    assert!(
        initial
            .iter()
            .any(|record| record.event["type"] == "model_request")
    );
    assert!(!follower.is_finished());
    release_tx.send(()).expect("release");
    worker.join().expect("worker");
    provider.join().expect("provider");
    let tail = follower.poll().expect("terminal records");
    assert!(
        tail.iter()
            .any(|record| record.event["type"] == "model_result")
    );
    assert!(follower.is_finished());
    let text = fs::read_to_string(journal).expect("journal text");
    assert!(!text.contains("private-value"));
    assert!(text.contains("[REDACTED]"));
}

#[test]
fn protected_guidance_interrupts_a_quiet_generation_without_replacing_the_task() {
    let dir = tempfile::tempdir().expect("workspace");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let controls = sav::runtime::ControlMailbox::default();
    let mailbox = controls.clone();
    let token = sav::CancellationToken::new();
    let (started_tx, started_rx) = mpsc::channel();
    let provider = thread::spawn(move || {
        let mut inference = 0;
        loop {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("timeout");
            let mut bytes = Vec::new();
            let mut chunk = [0_u8; 8192];
            loop {
                let count = stream.read(&mut chunk).expect("request");
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(split) = bytes.windows(4).position(|tail| tail == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..split]).to_ascii_lowercase();
                    let length = header
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:").map(str::trim))
                        .expect("length")
                        .parse::<usize>()
                        .expect("number");
                    if bytes.len() >= split + 4 + length {
                        break;
                    }
                }
            }
            if bytes.is_empty() {
                continue;
            }
            inference += 1;
            if inference == 1 {
                started_tx.send(()).expect("started");
                assert_eq!(stream.read(&mut chunk).expect("cancelled generation"), 0);
                continue;
            }
            let text = String::from_utf8(bytes).expect("request text");
            assert!(text.contains("original task"));
            assert!(text.contains("additional constraint"));
            let body = r#"{"model":"fixture","message":{"role":"assistant","content":"guided answer"},"done":true,"done_reason":"stop"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("reply");
            break;
        }
    });
    let workdir = dir.path().to_owned();
    let worker = thread::spawn(move || {
        kvist::agent::execute_agent_controlled(
            &profile(&endpoint),
            &SandboxConfig {
                runner: "/usr/bin/true".into(),
                backend: "/usr/bin/true".into(),
                environment_allowlist: vec![],
                acquisition: Default::default(),
            },
            None,
            None,
            AgentExecutionRequest {
                project_root: &workdir,
                vcs_selection: VcsSelection::Git,
                prompt: "original task",
                context_paths: &[],
                read_only_mounts: &[],
                target_dir: &workdir,
                task_id: "controlled-task",
                stream_output: false,
                role: Role::Developer,
                policy_identity: "sha256:fixture",
            },
            &mailbox,
            &token,
        )
    });
    started_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("quiet inference");
    let id = controls
        .steer("additional constraint")
        .expect("accepted guidance");
    assert!(worker.join().expect("worker").expect("run").success);
    provider.join().expect("provider");
    assert!(!controls.is_active());
    assert!(controls.steer("must not become work").is_err());
    let journal = fs::read_dir(dir.path().join(".kvist/runs"))
        .expect("runs")
        .map(|entry| entry.expect("entry").path())
        .find(|path| path.to_string_lossy().ends_with(".events.jsonl"))
        .expect("journal");
    let text = fs::read_to_string(journal).expect("journal");
    assert!(text.contains("\"state\":\"delivered\""));
    assert!(text.contains(&format!("\"id\":{}", id.0)));
}

#[test]
fn protected_tasks_refuse_opaque_agents_before_creating_execution_state() {
    let dir = tempfile::tempdir().expect("workspace");
    let mut profile = profile("not-a-gateway");
    profile.resolved_profile = Some(sav::registry::ResolvedProfile {
        name: "copilot".into(),
        profile: sav::registry::Profile::new(sav::registry::Backend::ExternalAgent(
            sav::registry::ExternalAgent {
                provider: sav::registry::ExternalProvider::Copilot,
                executable: "/not/to-be-executed".into(),
                arguments: vec!["{prompt}".into()],
                model: None,
            },
        )),
        source: dir.path().join("providers.toml"),
        digest: format!("sha256:{}", "0".repeat(64)),
    });
    let error = execute_agent(
        &profile,
        &SandboxConfig {
            runner: "/not/to-be-executed".into(),
            backend: "/not/to-be-executed".into(),
            environment_allowlist: vec![],
            acquisition: Default::default(),
        },
        None,
        None,
        AgentExecutionRequest {
            project_root: dir.path(),
            vcs_selection: VcsSelection::Git,
            prompt: "protected",
            context_paths: &[],
            read_only_mounts: &[],
            target_dir: dir.path(),
            task_id: "fixture",
            stream_output: false,
            role: Role::Developer,
            policy_identity: "fixture",
        },
    )
    .expect_err("external backend cannot receive protected task authority");
    assert!(error.to_string().contains("broker contract"));
    assert!(!dir.path().join(".kvist").exists());
}

fn read_request(stream: &mut std::net::TcpStream) -> Option<serde_json::Value> {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("deadline");
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = stream.read(&mut chunk).expect("read request");
        if count == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(split) = bytes.windows(4).position(|tail| tail == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&bytes[..split]).to_ascii_lowercase();
            let length = header
                .lines()
                .find_map(|line| line.strip_prefix("content-length:").map(str::trim))
                .expect("length")
                .parse::<usize>()
                .expect("number");
            if bytes.len() >= split + 4 + length {
                return Some(
                    serde_json::from_slice(&bytes[split + 4..split + 4 + length])
                        .expect("JSON body"),
                );
            }
        }
    }
}

fn accept_request(listener: &TcpListener) -> (std::net::TcpStream, serde_json::Value) {
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let started = Instant::now();
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "provider did not receive the expected inference request"
        );
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Some(request) = read_request(&mut stream) {
                    return (stream, request);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("accept provider connection: {error}"),
        }
    }
}

fn respond(stream: &mut std::net::TcpStream, input: u64, output: u64) {
    respond_with_tools(stream, input, output, serde_json::json!([]));
}

fn respond_with_effect(stream: &mut std::net::TcpStream, input: u64, output: u64) {
    respond_with_tools(
        stream,
        input,
        output,
        serde_json::json!([{"function":{"name":"write_file","arguments":{
            "destination":"src/observed.rs","content":"// fixture"
        }}}]),
    );
}

fn respond_with_tools(
    stream: &mut std::net::TcpStream,
    input: u64,
    output: u64,
    tools: serde_json::Value,
) {
    respond_with_tools_and_text(stream, input, output, tools, "observed answer");
}

fn respond_with_tools_and_text(
    stream: &mut std::net::TcpStream,
    input: u64,
    output: u64,
    tools: serde_json::Value,
    text: &str,
) {
    let body = serde_json::json!({
        "model":"fixture","message":{"role":"assistant","content":text,"tool_calls":tools},
        "done":true,"done_reason":"stop","prompt_eval_count":input,"eval_count":output
    })
    .to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("response");
}

struct EffectGate {
    _directory: tempfile::TempDir,
    listener: TcpListener,
    config: SandboxConfig,
    identity: kvist::sandbox::RunnerIdentity,
    probe: kvist::sandbox::SandboxProbe,
}

impl EffectGate {
    fn new(workdir: &Path) -> Self {
        // Keep the synthetic installed runner outside the selected worktree,
        // including when test scratch is deliberately placed inside the repo.
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(workdir)
                .status()
                .expect("isolated fixture worktree")
                .success()
        );
        fs::create_dir(workdir.join("src")).expect("authoring root");
        for artifact in [
            "REQUIREMENTS.md",
            "CONTRACT.md",
            "DESIGN.md",
            "TODOS.yaml",
            "IMPL.md",
        ] {
            fs::write(workdir.join(artifact), "fixture").expect("component context");
        }
        let directory = tempfile::tempdir().expect("external runner directory");
        let listener = TcpListener::bind("127.0.0.1:0").expect("effect gate");
        listener.set_nonblocking(true).expect("nonblocking gate");
        let runner = directory.path().join("runner");
        // This synthetic runner gates an effect; it does not claim isolation.
        fs::write(
            &runner,
            format!(
                r#"#!/bin/bash
set -eu
[ "$1" = "--kvist-sandbox-request-v1" ]
/bin/cat >/dev/null
exec 3<>/dev/tcp/127.0.0.1/{}
printf 'ready\n' >&3
IFS= read -r -t 10 release <&3
[ "$release" = "continue" ]
"#,
                listener.local_addr().expect("gate address").port()
            ),
        )
        .expect("runner");
        fs::set_permissions(&runner, fs::Permissions::from_mode(0o755)).expect("executable");
        let config = SandboxConfig {
            runner: runner.to_string_lossy().into_owned(),
            backend: "/usr/bin/true".into(),
            environment_allowlist: vec![],
            acquisition: Default::default(),
        };
        let identity = kvist::sandbox::runner_identity(&config, workdir, VcsSelection::Git)
            .expect("runner identity");
        let probe = kvist::sandbox::SandboxProbe {
            runner_path: identity.canonical_path.clone(),
            runner_digest: identity.digest.clone(),
            backend: kvist::sandbox::backend_identity(&config, workdir, VcsSelection::Git)
                .expect("backend identity"),
        };
        Self {
            _directory: directory,
            listener,
            config,
            identity,
            probe,
        }
    }

    fn release(&self, before_release: impl FnOnce()) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match self.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "effect was not dispatched");
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("accept effect gate: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("gate deadline");
        let mut ready = [0; 6];
        stream.read_exact(&mut ready).expect("effect started");
        assert_eq!(&ready, b"ready\n");
        before_release();
        stream.write_all(b"continue\n").expect("release effect");
    }
}

fn shared_profile(endpoint: &str, context_limit: usize) -> AgentProfile {
    let mut selected = profile(endpoint);
    let mut native = sav::registry::NativeModel::new(
        sav::LocalModelProvider::Ollama,
        endpoint.into(),
        "fixture".into(),
    );
    native.context_limit = Some(context_limit);
    native.response_reserve = Some(256);
    selected.resolved_profile = Some(sav::registry::ResolvedProfile {
        name: "shared-fixture".into(),
        profile: sav::registry::Profile::new(sav::registry::Backend::Native(native)),
        source: "providers.toml".into(),
        digest: format!("sha256:{}", "0".repeat(64)),
    });
    selected
}

fn resilience_run(
    workdir: &Path,
    answer: impl FnMut(usize, &serde_json::Value) -> serde_json::Value + Send,
) -> (kvist::agent::AgentRunResult, usize) {
    resilience_run_with_text(workdir, "observed answer", answer)
}

fn resilience_run_with_text(
    workdir: &Path,
    text: &str,
    answer: impl FnMut(usize, &serde_json::Value) -> serde_json::Value + Send,
) -> (kvist::agent::AgentRunResult, usize) {
    resilience_run_with_authority(workdir, text, answer, None)
}

fn resilience_run_with_authority(
    workdir: &Path,
    text: &str,
    mut answer: impl FnMut(usize, &serde_json::Value) -> serde_json::Value + Send,
    gate: Option<&EffectGate>,
) -> (kvist::agent::AgentRunResult, usize) {
    resilience_run_scripted(
        workdir,
        |index, request| (answer(index, request), text.to_owned()),
        gate,
    )
}

fn resilience_run_scripted(
    workdir: &Path,
    mut answer: impl FnMut(usize, &serde_json::Value) -> (serde_json::Value, String) + Send,
    gate: Option<&EffectGate>,
) -> (kvist::agent::AgentRunResult, usize) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    listener.set_nonblocking(true).expect("nonblocking");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let stopped = AtomicBool::new(false);
    thread::scope(|scope| {
        let stopped_ref = &stopped;
        let provider = scope.spawn(move || {
            let mut calls = 0;
            let deadline = Instant::now() + Duration::from_secs(30);
            while !stopped_ref.load(Ordering::SeqCst) {
                assert!(Instant::now() < deadline, "missing inference");
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let Some(request) = read_request(&mut stream) else {
                            continue;
                        };
                        calls += 1;
                        let (tools, text) = answer(calls, &request);
                        respond_with_tools_and_text(&mut stream, 10, 4, tools, &text);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            }
            calls
        });
        let mut selected = shared_profile(&endpoint, 32768);
        selected.timeout_seconds = 30;
        let no_effect_config = SandboxConfig {
            runner: "/usr/bin/true".into(),
            backend: "/usr/bin/true".into(),
            environment_allowlist: vec![],
            acquisition: Default::default(),
        };
        let result = execute_agent(
            &selected,
            gate.map_or(&no_effect_config, |gate| &gate.config),
            gate.map(|gate| &gate.identity),
            gate.map(|gate| &gate.probe),
            AgentExecutionRequest {
                project_root: workdir,
                vcs_selection: VcsSelection::Git,
                prompt: "finish the required implementation, not merely inspection",
                context_paths: &[],
                read_only_mounts: &[],
                target_dir: workdir,
                task_id: "resilience-task",
                stream_output: false,
                role: Role::Developer,
                policy_identity: "fixture",
            },
        )
        .expect("recorded outcome");
        stopped.store(true, Ordering::SeqCst);
        (result, provider.join().expect("provider"))
    })
}

fn read_call(path: &str) -> serde_json::Value {
    serde_json::json!([{"function":{"name":"read_file","arguments":{"path":path}}}])
}

#[test]
fn resilience_repeated_reads_are_stalled_not_completed() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    fs::write(dir.path().join("src/file.rs"), "pub fn incomplete() {}\n").expect("file");
    let (result, calls) = resilience_run(dir.path(), |_, _| read_call("src/file.rs"));
    assert!(!result.success, "a read loop is not completion");
    assert!(result.stderr.contains("stalled"), "{}", result.stderr);
    assert!(calls <= 6, "duplicate reads must terminate promptly");
}

#[test]
fn resilience_tool_only_repeated_reads_never_synthesize_an_answer() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    fs::write(dir.path().join("src/file.rs"), "// inspect only\n").expect("file");
    let (result, calls) = resilience_run_with_text(dir.path(), "", |_, _| read_call("src/file.rs"));
    assert!(!result.success);
    assert!(result.stderr.contains("stalled"), "{}", result.stderr);
    assert!(!result.stdout.contains("Completed"), "{}", result.stdout);
    assert!(calls <= 6);
}

#[test]
fn resilience_explicit_tool_only_completion_preserves_the_models_report() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let report = "Implementation complete: the approved task has been completed.";
    let (result, calls) = resilience_run_with_text(dir.path(), "", |_, _| {
        serde_json::json!([{
            "function":{"name":"propose_decision","arguments":{
                "summary":report,
                "why":"All required work is complete."
            }}
        }])
    });
    assert!(result.success, "{}", result.stderr);
    assert_eq!(result.stdout, report);
    assert_eq!(calls, 1);
}

#[test]
fn resilience_more_than_30_productive_reads_reach_explicit_completion() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    for index in 1..=35 {
        fs::write(
            dir.path().join(format!("src/file{index}.rs")),
            format!("// {index}\n"),
        )
        .expect("file");
    }
    let (result, calls) = resilience_run(dir.path(), |index, _| {
        if index <= 35 {
            read_call(&format!("src/file{index}.rs"))
        } else {
            serde_json::json!([])
        }
    });
    assert!(result.success, "{}", result.stderr);
    assert_eq!(calls, 36, "novel inspection must not hit a turn ceiling");
}

#[test]
fn resilience_external_change_reaches_the_model_in_a_covered_range() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    let path = dir.path().join("src/file.rs");
    fs::write(&path, "// old revision\n").expect("old file");
    let (result, calls) = resilience_run(dir.path(), |index, request| {
        if index == 2 {
            assert!(request.to_string().contains("old revision"));
            fs::write(&path, "// externally changed revision\n").expect("external edit");
        }
        if index == 3 {
            assert!(
                request.to_string().contains("externally changed revision"),
                "covered range hid new bytes: {request}"
            );
        }
        if index <= 2 {
            read_call("src/file.rs")
        } else {
            serde_json::json!([])
        }
    });
    assert!(result.success, "{}", result.stderr);
    assert_eq!(calls, 3);
}

#[test]
fn resilience_pseudo_read_errors_have_failed_outcomes_and_no_progress() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let passage = (0..150)
        .map(|index| format!("word{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let (result, _) = resilience_run_with_text(dir.path(), &passage, |index, request| {
        if index > 1 {
            let content = request["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .rev()
                .find(|message| message["role"] == "tool")
                .and_then(|message| message["content"].as_str())
                .expect("tool outcome");
            assert!(
                content.contains("status=Some(1)")
                    || (content.contains("status=None") && content.contains("not executed")),
                "{content}"
            );
        }
        if index <= 6 {
            read_call(&format!("src/missing{index}.rs"))
        } else {
            serde_json::json!([])
        }
    });
    assert!(!result.success, "novel errors are not observation progress");
    let journal = fs::read_dir(dir.path().join(".kvist/runs"))
        .expect("runs")
        .map(|entry| entry.expect("entry").path())
        .find(|path| path.to_string_lossy().ends_with(".events.jsonl"))
        .expect("journal");
    let records = fs::read_to_string(journal).expect("events");
    let results: Vec<serde_json::Value> = records
        .lines()
        .map(|line| serde_json::from_str(line).expect("event"))
        .filter(|record: &serde_json::Value| record["event"]["type"] == "tool_result")
        .collect();
    assert!(!results.is_empty());
    assert!(
        results
            .iter()
            .all(|record| record["event"]["data"]["applied"] == false
                || record["event"]["data"]["not_executed"] == true)
    );
}

#[test]
fn resilience_malformed_decision_is_not_an_explicit_completion_note() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let (result, _) = resilience_run(dir.path(), |_, _| {
        serde_json::json!([{
            "function":{"name":"propose_decision","arguments":{
                "_malformed_arguments_error":"truncated",
                "_malformed_arguments_preview":"unfinished"
            }}
        }])
    });
    assert!(!result.success, "malformed calls cannot certify completion");
    assert!(result.stderr.contains("stalled"), "{}", result.stderr);
}

#[test]
fn resilience_duplicate_feedback_describes_only_observed_bytes_without_forcing_writes() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    fs::write(dir.path().join("src/file.rs"), "// unchanged page\n").expect("file");
    let (result, calls) = resilience_run(dir.path(), |index, request| {
        if index == 3 {
            let content = request["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .rev()
                .find(|message| message["role"] == "tool")
                .and_then(|message| message["content"].as_str())
                .expect("tool result");
            assert!(content.contains("// unchanged page"), "{content}");
            assert!(!content.contains("contents have NOT changed"), "{content}");
            assert!(!content.contains("make required changes"), "{content}");
            serde_json::json!([])
        } else {
            read_call("src/file.rs")
        }
    });
    assert!(result.success, "{}", result.stderr);
    assert_eq!(calls, 3);
}

#[test]
fn resilience_conditional_completion_proposal_does_not_finish_authoring() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let (result, _) = resilience_run(dir.path(), |_, _| {
        serde_json::json!([{
            "function":{"name":"propose_decision","arguments":{
                "summary":"Need to verify before implementation complete",
                "why":"If tests have been added, I still need to inspect src and finish implementation."
            }}
        }])
    });
    assert!(
        !result.success,
        "a conditional completion phrase is not completion"
    );
}

#[test]
fn resilience_review_failed_read_and_edit_trajectory_results_are_faithful() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    fs::create_dir(dir.path().join("src")).expect("src");
    let contents = "pub fn kept() {}\n";
    fs::write(dir.path().join("src/file.rs"), contents).expect("file");
    let (result, calls) = resilience_run(dir.path(), |index, _| match index {
        1 => read_call("src/missing.rs"),
        2 => serde_json::json!([{"function":{"name":"edit_file","arguments":{
            "destination":"src/file.rs", "replace":"absent excerpt", "content":"changed"
        }}}]),
        3 => read_call("src/file.rs"),
        _ => serde_json::json!([]),
    });
    assert!(result.success, "{}", result.stderr);
    assert_eq!(calls, 4);
    assert_eq!(
        fs::read_to_string(dir.path().join("src/file.rs")).expect("unchanged"),
        contents
    );
    let trajectory = fs::read_dir(dir.path().join(".kvist/runs"))
        .expect("runs")
        .map(|entry| entry.expect("entry").path())
        .find(|path| path.to_string_lossy().ends_with(".trajectory.jsonl"))
        .expect("trajectory");
    let records = fs::read_to_string(trajectory).expect("trajectory records");
    let results: Vec<serde_json::Value> = records
        .lines()
        .map(|line| serde_json::from_str(line).expect("record"))
        .filter(|record: &serde_json::Value| record["event"] == "tool_result")
        .collect();
    assert_eq!(results.len(), 3, "{records}");
    assert_eq!(results[0]["tool"], "read_file");
    assert_eq!(results[1]["tool"], "edit_file");
    assert_eq!(results[0]["exit_code"], 1, "{records}");
    assert_eq!(results[1]["exit_code"], 1, "{records}");
    assert_eq!(results[2]["exit_code"], 0, "{records}");
    assert!(
        results
            .iter()
            .all(|record| record["state_mutated"] == false)
    );
}

#[test]
fn resilience_review_later_turn_choices_without_a_heading_stop_before_effects() {
    for alternative in ["Option A", "Option 1", "Alternative 1"] {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        let gate = EffectGate::new(dir.path());
        fs::write(dir.path().join("src/file.rs"), "// unchanged\n").expect("file");
        let (result, calls) = resilience_run_with_authority(
            dir.path(),
            "observed answer",
            |index, _| {
                if index == 1 {
                    return read_call("src/file.rs");
                }
                serde_json::json!([
                    {"function":{"name":"propose_decision","arguments":{
                        "summary":"An uncovered API choice requires human arbitration",
                        "why":format!("{alternative}: change the public API. Another choice: change the requirement. Neither is authorized.")
                    }}},
                    {"function":{"name":"write_file","arguments":{
                        "destination":"src/unauthorized.rs", "content":"pub fn forbidden() {}\n"
                    }}}
                ])
            },
            Some(&gate),
        );
        assert!(result.surfaced_decision, "{alternative}: {}", result.stderr);
        assert!(!result.success);
        assert_eq!(calls, 2);
        assert!(!dir.path().join("src/unauthorized.rs").exists());
        let journal = fs::read_dir(dir.path().join(".kvist/runs"))
            .expect("runs")
            .map(|entry| entry.expect("entry").path())
            .find(|path| path.to_string_lossy().ends_with(".events.jsonl"))
            .expect("journal");
        let records = fs::read_to_string(journal).expect("records");
        assert!(
            !records.lines().any(|line| {
                let record = serde_json::from_str::<serde_json::Value>(line).expect("event");
                let event = &record["event"];
                event["type"] == "tool_dispatch"
                    && matches!(
                        event["data"]["name"].as_str().expect("dispatch name"),
                        "write_file" | "edit_file"
                    )
            }),
            "{records}"
        );
    }
}

#[test]
fn resilience_review_partial_or_blocked_reports_are_not_completion() {
    for (summary, why) in [
        (
            "Tests have been added; implementation remains unfinished",
            "Source implementation remains absent",
        ),
        (
            "Tests implemented; the overall task is only partially complete",
            "Source implementation is still missing",
        ),
        (
            "Implementation complete except for an unresolved blocker",
            "Blocked on the remaining required behavior",
        ),
        (
            "Task has been completed except for required source work",
            "Required implementation remains outstanding",
        ),
    ] {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        let (result, _) = resilience_run_with_text(dir.path(), "", |_, _| {
            serde_json::json!([{
                "function":{"name":"propose_decision","arguments":{"summary":summary,"why":why}}
            }])
        });
        assert!(
            !result.success,
            "partial report completed authoring: {summary}"
        );
    }
}

#[test]
fn resilience_review_legitimate_whole_task_and_test_task_reports_still_complete() {
    for (summary, why) in [
        (
            "Implementation complete",
            "The approved task has been completed.",
        ),
        (
            "Tests have been added",
            "The approved write-tests task has been completed using the existing test framework.",
        ),
    ] {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        let (result, calls) = resilience_run_with_text(dir.path(), "", |_, _| {
            serde_json::json!([{
                "function":{"name":"propose_decision","arguments":{"summary":summary,"why":why}}
            }])
        });
        assert!(result.success, "{}", result.stderr);
        assert!(!result.surfaced_decision);
        assert_eq!(result.stdout, summary);
        assert_eq!(calls, 1);
    }
}

#[test]
fn resilience_review_explicit_final_blockers_fail_but_quoted_prior_errors_do_not() {
    for (text, expected_success) in [
        (
            "Implementation remains unfinished; required work is still missing.",
            false,
        ),
        (
            "Blocked on unavailable dependencies; unable to complete the task.",
            false,
        ),
        (
            "Task is incomplete; remaining work needs a resource.",
            false,
        ),
        ("There is no remaining work; implementation complete.", true),
        (
            "Implemented the change.\n> Prior output: implementation remains unfinished\nAll checks pass.",
            true,
        ),
        (
            "Implemented the change.\n```text\nimplementation remains unfinished\n```\nAll checks pass.",
            true,
        ),
        (
            "Implemented the change; prior output was \"implementation remains unfinished\". All checks pass.",
            true,
        ),
    ] {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        let (result, calls) =
            resilience_run_with_text(dir.path(), text, |_, _| serde_json::json!([]));
        assert_eq!(
            result.success, expected_success,
            "{text}: {}",
            result.stderr
        );
        assert_eq!(calls, 1);
    }
}

#[test]
fn resilience_review_summary_only_alternatives_retain_the_decision_pause() {
    for alternative in [
        "Option A",
        "Option 1",
        "Alternative 1",
        "Option B",
        "Option 2",
        "Alternative 2",
    ] {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        let gate = EffectGate::new(dir.path());
        fs::write(dir.path().join("src/file.rs"), "// unchanged\n").expect("file");
        let (result, calls) = resilience_run_with_authority(
            dir.path(),
            "observed answer",
            |index, _| {
                if index == 1 {
                    return read_call("src/file.rs");
                }
                serde_json::json!([
                    {"function":{"name":"propose_decision","arguments":{
                        "summary":format!("{alternative}: change the public API. Another choice: change the requirement."),
                        "why":"Need to verify public API; neither alternative is authorized."
                    }}},
                    {"function":{"name":"write_file","arguments":{
                        "destination":"src/unauthorized.rs", "content":"pub fn forbidden() {}\n"
                    }}}
                ])
            },
            Some(&gate),
        );
        assert!(result.surfaced_decision, "{alternative}: {}", result.stderr);
        assert!(!result.success);
        assert_eq!(calls, 2);
        assert!(!dir.path().join("src/unauthorized.rs").exists());
    }
}

#[test]
fn resilience_review_patch_blockers_cannot_be_discarded_as_completion() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let (result, calls) = resilience_run_with_text(dir.path(), "", |_, _| {
        serde_json::json!([{"function":{"name":"propose_decision","arguments":{
            "summary":"Implementation complete",
            "why":"The approved task has been completed.",
            "patch":"Implementation remains unfinished."
        }}}])
    });
    assert!(!result.success, "{}", result.stderr);
    assert!(result.surfaced_decision, "{}", result.stderr);
    assert_eq!(calls, 1);
}

#[test]
fn protected_run_continues_beyond_64_generations() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    listener.set_nonblocking(true).expect("nonblocking");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let gate = EffectGate::new(dir.path());
    let stopped = AtomicBool::new(false);
    let result = thread::scope(|scope| {
        let provider = scope.spawn(|| {
            let mut inference = 0;
            let mut last_request = Instant::now();
            loop {
                if stopped.load(Ordering::SeqCst) {
                    return inference;
                }
                assert!(
                    last_request.elapsed() < Duration::from_secs(10),
                    "missing model request"
                );
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if read_request(&mut stream).is_none() {
                            continue;
                        }
                        last_request = Instant::now();
                        inference += 1;
                        if inference <= 65 {
                            respond_with_tools(
                                &mut stream,
                                10,
                                4,
                                serde_json::json!([{
                                    "function":{"name":"write_file","arguments":{
                                        "destination":"src/observed.rs",
                                        "content":format!("// fixture revision {inference}\n")
                                    }}
                                }]),
                            );
                            gate.release(|| {
                                let staged = fs::read_dir(dir.path().join(".kvist/authoring"))
                                    .expect("staging")
                                    .map(|entry| entry.expect("entry").path())
                                    .find(|path| {
                                        path.extension()
                                            .is_some_and(|extension| extension == "json")
                                    })
                                    .expect("staged effect");
                                kvist::authoring::apply::apply_intent(dir.path(), &staged)
                                    .expect("actual authorized revision");
                            });
                        } else {
                            respond(&mut stream, 10, 4);
                            return inference;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            }
        });
        let mut selected = shared_profile(&endpoint, 8192);
        selected.timeout_seconds = 604_800;
        let result = execute_agent(
            &selected,
            &gate.config,
            Some(&gate.identity),
            Some(&gate.probe),
            AgentExecutionRequest {
                project_root: dir.path(),
                vcs_selection: VcsSelection::Git,
                prompt: "complete all 65 steps",
                context_paths: &[],
                read_only_mounts: &[],
                target_dir: dir.path(),
                task_id: "long-task",
                stream_output: false,
                role: Role::Developer,
                policy_identity: "fixture",
            },
        )
        .expect("recorded outcome");
        stopped.store(true, Ordering::SeqCst);
        assert!(result.success, "{}", result.stderr);
        let generations = provider.join().expect("provider");
        assert_eq!(generations, 66, "{}", result.stderr);
        result
    });
    assert!(result.success, "{}", result.stderr);
}

#[test]
fn multi_day_explicit_endpoint_run_accepts_repeated_human_guidance() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let controls = sav::runtime::ControlMailbox::default();
    let stopped = AtomicBool::new(false);
    let result = thread::scope(|scope| {
        let provider = scope.spawn(|| {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut inference = 0;
            while !stopped.load(Ordering::SeqCst) {
                assert!(Instant::now() < deadline, "missing request");
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let Some(request) = read_request(&mut stream) else {
                            continue;
                        };
                        inference += 1;
                        if inference <= 5 {
                            controls
                                .steer(format!("additional guidance {inference}"))
                                .unwrap();
                            assert_eq!(stream.read(&mut [0]).unwrap(), 0, "generation interrupted");
                        } else {
                            assert!(request.to_string().contains("additional guidance 5"));
                            respond(&mut stream, 10, 4);
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            }
            inference
        });
        let mut selected = profile(&endpoint);
        selected.timeout_seconds = 604_800;
        let result = kvist::agent::execute_agent_controlled(
            &selected,
            &SandboxConfig {
                runner: "/usr/bin/true".into(),
                backend: "/usr/bin/true".into(),
                environment_allowlist: vec![],
                acquisition: Default::default(),
            },
            None,
            None,
            AgentExecutionRequest {
                project_root: dir.path(),
                vcs_selection: VcsSelection::Git,
                prompt: "original task",
                context_paths: &[],
                read_only_mounts: &[],
                target_dir: dir.path(),
                task_id: "long-guided-task",
                stream_output: false,
                role: Role::Developer,
                policy_identity: "fixture",
            },
            &controls,
            &sav::CancellationToken::new(),
        )
        .unwrap();
        stopped.store(true, Ordering::SeqCst);
        let calls = provider.join().unwrap();
        assert_eq!(calls, 6, "{}", result.stderr);
        result
    });
    assert!(result.success, "{}", result.stderr);
}

#[test]
fn approved_context_reaches_native_and_explicit_endpoint_requests() {
    for shared in [true, false] {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        let root = dir.path().canonicalize().expect("absolute workspace");
        fs::write(root.join("REQUIREMENTS.md"), "local-context-marker").expect("local");
        fs::write(root.join("ROOT_CONTRACT.md"), "root-context-marker").expect("root");
        fs::write(root.join("PARENT_CONTRACT.md"), "parent-context-marker").expect("parent");
        fs::write(root.join("peer.rs"), "never-include-peer-marker").expect("peer");
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let provider = thread::spawn(move || {
            loop {
                let (mut stream, _) = listener.accept().expect("accept");
                if let Some(request) = read_request(&mut stream) {
                    let text = request.to_string();
                    for marker in [
                        "local-context-marker",
                        "root-context-marker",
                        "parent-context-marker",
                        "fixture task",
                    ] {
                        assert!(text.contains(marker), "missing selected context: {marker}");
                    }
                    assert!(!text.contains("never-include-peer-marker"));
                    respond(&mut stream, 10, 4);
                    break;
                }
            }
        });
        let selected = if shared {
            shared_profile(&endpoint, 8192)
        } else {
            profile(&endpoint)
        };
        let context = [
            "/workspace/component/REQUIREMENTS.md".into(),
            "/workspace/context/ROOT_CONTRACT.md".into(),
            "/workspace/context/PARENT_CONTRACT.md".into(),
        ];
        let mounts = [
            kvist::sandbox::ReadOnlyMount::file(
                root.join("ROOT_CONTRACT.md"),
                "/workspace/context/ROOT_CONTRACT.md",
            ),
            kvist::sandbox::ReadOnlyMount::file(
                root.join("PARENT_CONTRACT.md"),
                "/workspace/context/PARENT_CONTRACT.md",
            ),
        ];
        let result = execute_agent(
            &selected,
            &SandboxConfig {
                runner: "/usr/bin/true".into(),
                backend: "/usr/bin/true".into(),
                environment_allowlist: vec![],
                acquisition: Default::default(),
            },
            None,
            None,
            AgentExecutionRequest {
                project_root: &root,
                vcs_selection: VcsSelection::Git,
                prompt: "fixture task",
                context_paths: &context,
                read_only_mounts: &mounts,
                target_dir: &root,
                task_id: "context-task",
                stream_output: false,
                role: Role::Developer,
                policy_identity: "fixture",
            },
        );
        provider.join().expect("provider");
        assert!(result.expect("run").success);
    }
}

#[test]
fn steering_retains_whole_run_usage_and_cumulative_token_limit() {
    for limited in [false, true] {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let mailbox = sav::runtime::ControlMailbox::default();
        let controls = mailbox.clone();
        let gate = EffectGate::new(dir.path());
        let gate_ref = &gate;
        let result = thread::scope(|scope| {
            let provider = scope.spawn(move || {
                let mut inference = 0;
                loop {
                    let (mut stream, request) = accept_request(&listener);
                    inference += 1;
                    if inference == 1 {
                        respond_with_effect(&mut stream, if limited { 6500 } else { 11 }, 7);
                        gate_ref.release(|| {
                            controls.steer("continue with guidance").expect("guidance");
                        });
                        if limited {
                            break;
                        }
                    } else {
                        assert!(request.to_string().contains("continue with guidance"));
                        respond(&mut stream, 13, 5);
                        break;
                    }
                }
            });
            let mut selected = shared_profile(&endpoint, 8192);
            // This tests accounting, not durable-recording latency under parallel load.
            selected.timeout_seconds = 60;
            selected.token_limit = limited.then_some(7000);
            let result = kvist::agent::execute_agent_controlled(
                &selected,
                &gate.config,
                Some(&gate.identity),
                Some(&gate.probe),
                AgentExecutionRequest {
                    project_root: dir.path(),
                    vcs_selection: VcsSelection::Git,
                    prompt: "fixture task",
                    context_paths: &[],
                    read_only_mounts: &[],
                    target_dir: dir.path(),
                    task_id: "usage-task",
                    stream_output: false,
                    role: Role::Developer,
                    policy_identity: "fixture",
                },
                &mailbox,
                &sav::CancellationToken::new(),
            )
            .expect("recorded outcome");
            provider.join().expect("provider");
            result
        });
        assert_eq!(result.success, !limited);
        assert_eq!(result.tokens_input, Some(if limited { 6500 } else { 24 }));
        assert_eq!(result.tokens_output, Some(if limited { 7 } else { 12 }));
        if limited {
            assert!(result.stderr.contains("token"), "{}", result.stderr);
        }
        let trajectory = fs::read_dir(dir.path().join(".kvist/runs"))
            .expect("runs")
            .map(|entry| entry.expect("entry").path())
            .find(|path| path.to_string_lossy().ends_with(".trajectory.jsonl"))
            .expect("trajectory");
        let replay = sav::replay_trajectory(&trajectory, None).expect("replay");
        assert!(replay.events.iter().any(|event| matches!(
            event,
            sav::TrajectoryEvent::SessionFinish { total_tokens, .. }
                if *total_tokens == if limited {6507} else {36}
        )));
    }
}

#[test]
fn steering_before_usage_is_reported_keeps_unknown_attempt_separate() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let mailbox = sav::runtime::ControlMailbox::default();
    let controls = mailbox.clone();
    let provider = thread::spawn(move || {
        let (mut stream, _) = accept_request(&listener);
        controls.steer("continue").expect("guidance");
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte).expect("interrupted response"), 0);
        let (mut stream, request) = accept_request(&listener);
        assert!(request.to_string().contains("continue"));
        respond(&mut stream, 10, 4);
    });
    let mut selected = shared_profile(&endpoint, 8192);
    selected.timeout_seconds = 60;
    selected.token_limit = Some(7000);
    let result = kvist::agent::execute_agent_controlled(
        &selected,
        &SandboxConfig {
            runner: "/usr/bin/true".into(),
            backend: "/usr/bin/true".into(),
            environment_allowlist: vec![],
            acquisition: Default::default(),
        },
        None,
        None,
        AgentExecutionRequest {
            project_root: dir.path(),
            vcs_selection: VcsSelection::Git,
            prompt: "fixture task",
            context_paths: &[],
            read_only_mounts: &[],
            target_dir: dir.path(),
            task_id: "unknown-usage",
            stream_output: false,
            role: Role::Developer,
            policy_identity: "fixture",
        },
        &mailbox,
        &sav::CancellationToken::new(),
    )
    .expect("recorded outcome");
    provider.join().expect("provider");
    assert!(result.success, "{}", result.stderr);
    assert_eq!(result.tokens_input, Some(10));
    assert_eq!(result.tokens_output, Some(4));
    let log = fs::read_to_string(result.log_path).expect("log");
    assert!(log.contains("conservatively charged/unknown attempts=1"));
}

#[test]
fn forbidden_native_endpoint_is_refused_before_liveness_connect() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let listener = TcpListener::bind("0.0.0.0:0").expect("listener");
    listener.set_nonblocking(true).expect("nonblocking");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let result = execute_agent(
        &shared_profile(&endpoint, 8192),
        &SandboxConfig {
            runner: "/usr/bin/true".into(),
            backend: "/usr/bin/true".into(),
            environment_allowlist: vec![],
            acquisition: Default::default(),
        },
        None,
        None,
        AgentExecutionRequest {
            project_root: dir.path(),
            vcs_selection: VcsSelection::Git,
            prompt: "fixture",
            context_paths: &[],
            read_only_mounts: &[],
            target_dir: dir.path(),
            task_id: "forbidden-endpoint",
            stream_output: false,
            role: Role::Developer,
            policy_identity: "fixture",
        },
    );
    assert!(result.is_err() || !result.expect("outcome").success);
    assert_eq!(
        listener
            .accept()
            .expect_err("must not probe forbidden authority")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn cancelled_generation_keeps_earlier_turn_evidence_and_usage() {
    let dir = tempfile::tempdir_in(".").expect("workspace");
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let endpoint = format!("http://{}", listener.local_addr().expect("address"));
    let mailbox = sav::runtime::ControlMailbox::default();
    let gate = EffectGate::new(dir.path());
    let gate_ref = &gate;
    let token = sav::CancellationToken::new();
    let cancel = token.clone();
    thread::scope(|scope| {
        let provider = scope.spawn(move || {
            let mut inference = 0;
            loop {
                let (mut stream, _) = accept_request(&listener);
                inference += 1;
                if inference == 1 {
                    respond_with_effect(&mut stream, 11, 7);
                    gate_ref.release(|| {});
                } else {
                    cancel.cancel();
                    break;
                }
            }
        });
        let mut selected = shared_profile(&endpoint, 8192);
        selected.timeout_seconds = 60;
        let error = kvist::agent::execute_agent_controlled(
            &selected,
            &gate.config,
            Some(&gate.identity),
            Some(&gate.probe),
            AgentExecutionRequest {
                project_root: dir.path(),
                vcs_selection: VcsSelection::Git,
                prompt: "original task",
                context_paths: &[],
                read_only_mounts: &[],
                target_dir: dir.path(),
                task_id: "cancel-evidence",
                stream_output: false,
                role: Role::Developer,
                policy_identity: "fixture",
            },
            &mailbox,
            &token,
        )
        .expect_err("cancellation must not become a success-shaped result");
        assert!(error.to_string().contains("cancel"));
        provider.join().expect("provider");
    });
    let log = fs::read_dir(dir.path().join(".kvist/logs"))
        .expect("logs")
        .next()
        .expect("log")
        .expect("entry")
        .path();
    let text = fs::read_to_string(log).expect("log");
    assert!(text.contains("observed answer"));
    assert!(text.contains("known input=Some(11)"));
    assert!(text.contains("known output=Some(7)"));
    assert!(text.contains("cancel"));
    let trajectory = fs::read_dir(dir.path().join(".kvist/runs"))
        .expect("runs")
        .map(|entry| entry.expect("entry").path())
        .find(|path| path.to_string_lossy().ends_with(".trajectory.jsonl"))
        .expect("trajectory");
    let replay = sav::replay_trajectory(&trajectory, None).expect("replay");
    assert_eq!(replay.final_success, Some(false));
    assert!(replay.events.iter().any(|event| matches!(
        event,
        sav::TrajectoryEvent::SessionFinish {
            total_tokens: 18,
            ..
        }
    )));
}

#[test]
fn selected_context_that_cannot_fit_refuses_inference_instead_of_omitting_it() {
    for shared in [true, false] {
        let dir = tempfile::tempdir_in(".").expect("workspace");
        fs::write(
            dir.path().join("REQUIREMENTS.md"),
            "selected artifact ".repeat(4096),
        )
        .expect("selected artifact");
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        listener.set_nonblocking(true).expect("nonblocking");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let selection = if shared {
            shared_profile(&endpoint, 8192)
        } else {
            profile(&endpoint)
        };
        let context = ["/workspace/component/REQUIREMENTS.md".into()];
        let error = execute_agent(
            &selection,
            &SandboxConfig {
                runner: "/usr/bin/true".into(),
                backend: "/usr/bin/true".into(),
                environment_allowlist: vec![],
                acquisition: Default::default(),
            },
            None,
            None,
            AgentExecutionRequest {
                project_root: dir.path(),
                vcs_selection: VcsSelection::Git,
                prompt: "fixture task",
                context_paths: &context,
                read_only_mounts: &[],
                target_dir: dir.path(),
                task_id: "context-refusal",
                stream_output: false,
                role: Role::Developer,
                policy_identity: "fixture",
            },
        )
        .expect_err("cannot fit selected context");
        assert!(error.to_string().contains("context"), "{error}");
        assert_eq!(
            listener.accept().expect_err("no inference or probe").kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
