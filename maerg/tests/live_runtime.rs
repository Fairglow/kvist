use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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
            let mut request = [0_u8; 8192];
            let count = stream.read(&mut request).expect("read");
            if count == 0 {
                continue;
            }

            started_tx.send(()).expect("started");
            release_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("release");
            let body = serde_json::json!({
                "model":"fixture","message":{"role":"assistant","content":"done private-value"},
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
