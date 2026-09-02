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

#[test]
#[cfg(target_os = "linux")]
fn execute_agent_captures_stdout_and_stderr_in_log_file() {
    let workspace = TempDir::new().expect("workspace");
    let status = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(workspace.path())
        .status()
        .expect("initialize Git");
    assert!(status.success());

    // We use a basic command available on standard platforms like 'echo'
    let profile = AgentProfile {
        command_template: "/usr/bin/echo '{prompt}'".to_owned(),
        models: vec![Model {
            name: "default".to_owned(),
            command: "/usr/bin/echo '{prompt}'".to_owned(),
            system_prompt: None,
        }],
        default_model: "default".to_owned(),
        model: None,
        token_limit: None,
        timeout_seconds: 5,
        max_output_bytes: 1_024,
        redaction_values: vec![],
    };

    let prompt = "hello external agent";
    let context_paths: Vec<PathBuf> = vec![];
    let target_dir = workspace.path();
    let task_id = "test-task";
    let runner_workspace = TempDir::new().expect("external runner workspace");
    let runner = runner_workspace.path().join("fake-sandbox-runner");
    // Single-file runner: emits the version-one JSON probe on the probe argument
    // and a deterministic response for a request. It ignores the request body.
    fs::write(
        &runner,
        r#"#!/usr/bin/bash
set -eu
if [ "${1-}" = "--kvist-sandbox-probe-v1" ]; then
  runner_digest=$(sha256sum "$0" | cut -d' ' -f1)
  backend_digest=$(sha256sum /usr/bin/true | cut -d' ' -f1)
  printf '{"protocol":"kvist-sandbox-probe-v1","protocol_version":1,"runner":{"path":"%s","digest":"sha256:%s"},"backend":{"kind":"bubblewrap","path":"/usr/bin/true","digest":"sha256:%s"},"capabilities":{"namespaces":{"mount":true,"network":true,"pid":true,"ipc":true,"uts":true,"user":true},"new_session":true,"parent_death_signal":true}}\n' "$0" "$runner_digest" "$backend_digest"
  exit 0
fi
cat >/dev/null
printf 'sandboxed agent output\n'
"#,
    )
    .expect("write runner");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
        .expect("make runner executable");
    let sandbox = SandboxConfig {
        runner: runner.to_string_lossy().into_owned(),
        backend: "/usr/bin/true".to_owned(),
        environment_allowlist: vec![],
    };
    // The secure authoring boundary grants only explicit writable roots.
    fs::create_dir_all(target_dir.join("tests")).expect("create authoring test root");
    for document in [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        fs::write(target_dir.join(document), format!("fixture {document}\n"))
            .expect("write component context document");
    }
    let runner_identity = kvist::sandbox::runner_identity(&sandbox, target_dir, VcsSelection::Git)
        .expect("runner identity");
    let backend_identity =
        kvist::sandbox::backend_identity(&sandbox, target_dir, VcsSelection::Git)
            .expect("backend identity");
    let probe = kvist::sandbox::ensure_available(
        &sandbox,
        target_dir,
        VcsSelection::Git,
        &runner_identity,
        &backend_identity,
    )
    .expect("sandbox probe");

    let result = execute_agent(
        &profile,
        &sandbox,
        &runner_identity,
        &probe,
        kvist::agent::AgentExecutionRequest {
            project_root: target_dir,
            vcs_selection: VcsSelection::Git,
            prompt,
            context_paths: &context_paths,
            read_only_mounts: &[],
            target_dir,
            task_id,
            stream_output: false,
            role: Role::Developer,
            policy_identity: "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        },
    )
    .expect("agent execution success");

    assert!(result.success);
    assert_eq!(result.tokens_input, None);
    assert_eq!(result.tokens_output, None);

    // Verify log file exists and contains the echoed prompt
    assert!(result.log_path.exists());
    let log_contents = fs::read_to_string(&result.log_path).expect("read log contents");
    assert!(log_contents.contains("sandboxed agent output"));
}

#[cfg(target_os = "linux")]
fn probe_test_workspace(probe_body: &str) -> (TempDir, TempDir, SandboxConfig) {
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
