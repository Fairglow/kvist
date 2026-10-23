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
    let worker = thread::spawn(move || invoke(&workdir, &profile(&endpoint)));
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
