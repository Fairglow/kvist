use std::path::{Path, PathBuf};

use kvist::agent::split_command;

#[cfg(unix)]
use kvist::{
    agent::execute_agent,
    config::{AgentProfile, SandboxConfig, VcsSelection},
};
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use tempfile::TempDir;

#[test]
fn split_command_interpolates_placeholders_and_trims_quotes_correctly() {
    let template =
        "my-agent --message '{prompt}' --files {context_files} --dir '{target_directory}'";
    let prompt = "Implement user authentication";
    let context_paths = vec![
        PathBuf::from("/workspace/src/SPEC.md"),
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
            "/workspace/src/SPEC.md".to_owned(),
            "/workspace/src/TODOS.yaml".to_owned(),
            "--dir".to_owned(),
            "/workspace/src".to_owned(),
        ]
    );
}

#[test]
#[cfg(unix)]
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
        command_template: "echo '{prompt}'".to_owned(),
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
    fs::write(
        &runner,
        "#!/bin/sh\nif [ \"$1\" = --kvist-sandbox-probe-v1 ]; then\n  printf 'kvist-sandbox-probe-v1: network=deny; mount=component\\n'\nelse\n  cat >/dev/null\n  printf 'sandboxed agent output\\n'\nfi\n",
    )
    .expect("write runner");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
        .expect("make runner executable");
    let sandbox = SandboxConfig {
        runner: runner.to_string_lossy().into_owned(),
        environment_allowlist: vec![],
    };
    let runner_identity = kvist::sandbox::runner_identity(&sandbox, target_dir, VcsSelection::Git)
        .expect("runner identity");

    let result = execute_agent(
        &profile,
        &sandbox,
        &runner_identity,
        kvist::agent::AgentExecutionRequest {
            project_root: target_dir,
            vcs_selection: VcsSelection::Git,
            prompt,
            context_paths: &context_paths,
            target_dir,
            task_id,
            stream_output: false,
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
