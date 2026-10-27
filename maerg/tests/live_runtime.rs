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
    let body = serde_json::json!({
        "model":"fixture","message":{"role":"assistant","content":"observed answer","tool_calls":tools},
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
