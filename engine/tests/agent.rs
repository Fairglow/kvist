use std::path::{Path, PathBuf};

use kvist::agent::split_command;

#[cfg(target_os = "linux")]
use kvist::config::{AgentProfile, Model, Role};

#[cfg(target_os = "linux")]
use kvist::{
    agent::execute_agent,
    config::{SandboxConfig, VcsSelection},
};
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use tempfile::TempDir;

#[test]
fn split_command_interpolates_placeholders_and_trims_quotes_correctly() {
    let template =
        "my-agent --message '{prompt}' --files {context_files} --dir '{target_directory}'";
    let prompt = "Implement user authentication";
    let context_paths = vec![
        PathBuf::from("/workspace/src/REQUIREMENTS.md"),
        PathBuf::from("/workspace/src/TODOS.yaml"),
    ];
    let target_dir = Path::new("/workspace/src");

    let (program, args) = split_command(template, prompt, &context_paths, target_dir)
        .expect("successful split and interpolation");

    assert_eq!(program, "my-agent");
    assert_eq!(
        args,
        vec![
            "--message".to_owned(),
            "Implement user authentication".to_owned(),
            "--files".to_owned(),
            "/workspace/src/REQUIREMENTS.md".to_owned(),
            "/workspace/src/TODOS.yaml".to_owned(),
            "--dir".to_owned(),
            "/workspace/src".to_owned(),
        ]
    );
}

#[test]
fn split_command_omits_an_empty_context_option_pair() {
    let (program, args) = split_command(
        "my-agent --message '{prompt}' --context '{context_files}'",
        "Hello",
        &[],
        Path::new("."),
    )
    .expect("split command without context");

    assert_eq!(program, "my-agent");
    assert_eq!(args, vec!["--message", "Hello"]);
}

#[test]
fn split_command_preserves_quoted_paths_and_arguments() {
    let (program, args) = split_command(
        r#""C:\Program Files\Agent\agent.exe" --label "value with spaces" '{prompt}'"#,
        "Hello from Kvist",
        &[],
        Path::new("."),
    )
    .expect("split quoted command");

    assert_eq!(program, r"C:\Program Files\Agent\agent.exe");
    assert_eq!(
        args,
        vec!["--label", "value with spaces", "Hello from Kvist"]
    );
}

#[test]
fn split_command_decodes_escaped_backslashes_and_quotes() {
    let (program, args) = split_command(
        r#""C:\\Program Files\\Agent\\agent.exe" "quote: \"ready\"""#,
        "unused",
        &[],
        Path::new("."),
    )
    .expect("split escaped command");

    assert_eq!(program, r"C:\Program Files\Agent\agent.exe");
    assert_eq!(args, vec![r#"quote: "ready""#]);
}

/// Serves Ollama-style unary chat responses from a numeric loopback endpoint.
///
/// The brokered transport performs the model turn on the host, so a local
/// provider is all the test needs; no sandbox runner or probe is involved.
#[cfg(target_os = "linux")]
fn serve_local_ollama(content: &str) -> String {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    let listener = Arc::new(TcpListener::bind("127.0.0.1:0").expect("bind fake provider"));
    let addr = listener.local_addr().expect("fake provider address");
    let body = format!(
        "{{\"model\":\"test-model\",\"created_at\":\"2026-08-30T00:00:00Z\",\"message\":{{\"role\":\"assistant\",\"content\":\"{content}\",\"tool_calls\":[]}},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":10,\"eval_count\":4}}"
    );
    // A real model gateway serves many sequential connections: a brokered host
    // turn liveness-probes the port first, then performs the turn, so the fake
    // provider must serve more than one connection. Each worker accepts and
    // serves a single connection; any connection the client never opens simply
    // blocks on accept until the process exits.
    for _ in 0..4 {
        let listener = Arc::clone(&listener);
        let body = body.clone();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
            let _ = stream.flush();
        });
    }
    format!("http://{addr}/api/chat")
}

#[test]
#[cfg(target_os = "linux")]
fn execute_agent_runs_the_model_turn_on_the_host_and_captures_the_response() {
    kvist::init_test_logging();
    let target_dir = TempDir::new().expect("workspace");

    // A local Ollama-style provider stands in for the host model service.
    let endpoint = serve_local_ollama("brokered host turn ok");
    let prompt = "Read the approved contract";
    let command = format!(
        "curl --silent --request POST --json '{{\"model\":\"test-model\"}}' -- \"{endpoint}\""
    );

    // The profile command embeds the numeric loopback endpoint; the brokered
    // transport extracts it and performs the turn on the host.
    let profile = AgentProfile {
        role: Role::Developer,
        profile: "default".to_owned(),
        command_template: command.clone(),
        models: vec![Model {
            name: "default".to_owned(),
            command,
            system_prompt: None,
        }],
        default_model: "default".to_owned(),
        model: None,
        thinking_effort: None,
        token_limit: None,
        timeout_seconds: 5,
        max_output_bytes: 1_024,
        redaction_values: vec![],
    };

    let context_paths: Vec<PathBuf> = vec![];
    let task_id = "test-task";
    let sandbox = SandboxConfig {
        runner: "/usr/bin/true".to_owned(),
        backend: "/usr/bin/true".to_owned(),
        environment_allowlist: vec![],
        acquisition: kvist::config::AcquisitionConfig::default(),
    };

    let result = execute_agent(
        &profile,
        &sandbox,
        // A loopback host turn needs no authoring sandbox; the runner and
        // probe are None so no authoring request is ever dispatched.
        None,
        None,
        kvist::agent::AgentExecutionRequest {
            project_root: target_dir.path(),
            vcs_selection: VcsSelection::Git,
            prompt,
            context_paths: &context_paths,
            read_only_mounts: &[],
            target_dir: target_dir.path(),
            task_id,
            stream_output: false,
            role: Role::Developer,
            policy_identity: "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        },
    )
    .expect("agent execution success");

    assert!(result.success);
    // The provider response carries usage (`prompt_eval_count: 10`,
    // `eval_count: 4`); the brokered path must surface it, not discard it.
    assert_eq!(result.tokens_input, Some(10));
    assert_eq!(result.tokens_output, Some(4));

    // Verify the log file exists and contains the model turn response.
    assert!(result.log_path.exists());
    let log_contents = fs::read_to_string(&result.log_path).expect("read log contents");
    assert!(
        log_contents.contains("brokered host turn ok"),
        "log contents were: {log_contents}"
    );
}

#[cfg(target_os = "linux")]
fn probe_test_workspace(probe_body: &str) -> (TempDir, TempDir, SandboxConfig) {
    kvist::init_test_logging();
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let workspace = TempDir::new().expect("probe workspace");
    assert!(
        Command::new("git")
            .arg("init")
            .current_dir(workspace.path())
            .status()
            .expect("git init")
            .success()
    );
    let runner_workspace = TempDir::new().expect("runner workspace");
    let runner = runner_workspace.path().join("fake-probe-runner");
    fs::write(&runner, probe_body).expect("write probe runner");
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
        .expect("make runner executable");
    let sandbox = SandboxConfig {
        runner: runner.to_string_lossy().into_owned(),
        backend: "/usr/bin/true".to_owned(),
        environment_allowlist: vec![],
        acquisition: kvist::config::AcquisitionConfig::default(),
    };
    (workspace, runner_workspace, sandbox)
}

#[test]
#[cfg(target_os = "linux")]
fn ensure_available_rejects_a_probe_that_overflows_the_output_bound() {
    // The probe emits far more than the 64 KiB probe bound on standard output
    // and exits success; the bounded probe must reject it without an unbounded
    // capture.
    let (workspace, _runner_workspace, sandbox) = probe_test_workspace(
        r#"#!/usr/bin/bash
set -eu
if [ "${1-}" = "--kvist-sandbox-probe-v1" ]; then
  i=0
  while [ "$i" -lt 200000 ]; do
    printf 'AAAAAAAAAAAAAAAA'
    i=$((i + 1))
  done
  exit 0
fi
exit 3
"#,
    );
    let target = workspace.path();
    let runner_identity =
        kvist::sandbox::runner_identity(&sandbox, target, VcsSelection::Git).expect("runner id");
    let backend_identity =
        kvist::sandbox::backend_identity(&sandbox, target, VcsSelection::Git).expect("backend id");
    let error = kvist::sandbox::ensure_available(
        &sandbox,
        target,
        VcsSelection::Git,
        &runner_identity,
        &backend_identity,
    )
    .expect_err("an overflowing probe must be rejected");
    assert!(
        error.to_string().contains("size bound"),
        "diagnostic must be actionable: {error}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn ensure_available_rejects_a_successful_probe_that_writes_standard_error() {
    // A confirmed probe must emit only its JSON on standard output; a success
    // that also writes standard error is rejected.
    let (workspace, _runner_workspace, sandbox) = probe_test_workspace(
        r#"#!/usr/bin/bash
set -eu
if [ "${1-}" = "--kvist-sandbox-probe-v1" ]; then
  runner_digest=$(sha256sum "$0" | cut -d' ' -f1)
  backend_digest=$(sha256sum /usr/bin/true | cut -d' ' -f1)
  printf '{"protocol":"kvist-sandbox-probe-v1","protocol_version":1,"runner":{"path":"%s","digest":"sha256:%s"},"backend":{"kind":"bubblewrap","path":"/usr/bin/true","digest":"sha256:%s"},"capabilities":{"namespaces":{"mount":true,"network":true,"pid":true,"ipc":true,"uts":true,"user":true},"new_session":true,"parent_death_signal":true}}\n' "$0" "$runner_digest" "$backend_digest"
  printf 'unexpected diagnostic\n' >&2
  exit 0
fi
exit 3
"#,
    );
    let target = workspace.path();
    let runner_identity =
        kvist::sandbox::runner_identity(&sandbox, target, VcsSelection::Git).expect("runner id");
    let backend_identity =
        kvist::sandbox::backend_identity(&sandbox, target, VcsSelection::Git).expect("backend id");
    let error = kvist::sandbox::ensure_available(
        &sandbox,
        target,
        VcsSelection::Git,
        &runner_identity,
        &backend_identity,
    )
    .expect_err("a probe that writes stderr on success must be rejected");
    assert!(
        error.to_string().contains("standard error"),
        "diagnostic must be actionable: {error}"
    );
}
