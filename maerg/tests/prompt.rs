#![cfg(unix)]

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use tempfile::TempDir;

use kvist::prompt_input::MAX_PROMPT_BYTES;

fn configured_project() -> TempDir {
    let project = TempDir::new().expect("create project");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"

[agent.profiles.developer]
model = "capture"
default_model = "capture"
models = [{ name = "capture", command = "/bin/echo '{prompt}'" }]
"#,
    )
    .expect("write configuration");
    project
}

/// Installs a stub `agent-runner` that records its arguments to `log` and
/// emits one version-one NDJSON run-summary line, so the delegation contract
/// can be asserted without a live model. Returns the stub path.
fn install_stub_agent_runner(project: &Path, log: &Path) -> PathBuf {
    let stub = project.join("stub-agent-runner");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > {log}\nprintf \
         '{{\"schema_version\":1,\"sequence\":1,\"event\":\"run-summary\"}}\\n'\nexit 0\n",
        log = log.display()
    );
    fs::write(&stub, script).expect("write stub agent-runner");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o700)).expect("make stub executable");
    stub
}

fn run_kvist_in(project: &Path, args: &[&str], stub: Option<&Path>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kvist"));
    command.current_dir(project);
    for argument in args {
        command.arg(argument);
    }
    if let Some(stub) = stub {
        command.env("KVIST_AGENT_RUNNER", stub);
    }
    command
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .output()
        .expect("run prompt command")
}

#[test]
fn prompt_headless_delegates_to_agent_runner() {
    let project = configured_project();
    let log = project.path().join("stub-args.txt");
    let stub = install_stub_agent_runner(project.path(), &log);
    let prompt_path = project.path().join("prompt.md");
    fs::write(&prompt_path, "Prompt loaded from a file").expect("write prompt");
    let prompt_path = prompt_path.to_string_lossy().into_owned();

    let output = run_kvist_in(
        project.path(),
        &["prompt", "--file", &prompt_path],
        Some(&stub),
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recorded = fs::read_to_string(&log).expect("read stub arguments");
    let lines: Vec<&str> = recorded.lines().collect();
    assert_eq!(
        lines,
        vec![
            "--headless",
            "--json",
            "--model",
            "capture",
            "Prompt loaded from a file"
        ]
    );
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 output")
            .contains("run-summary")
    );
}

#[test]
fn prompt_headless_uses_explicit_model_and_effort() {
    let project = configured_project();
    let log = project.path().join("stub-args.txt");
    let stub = install_stub_agent_runner(project.path(), &log);

    let output = run_kvist_in(
        project.path(),
        &[
            "prompt",
            "--model",
            "capture",
            "--reasoning-effort",
            "high",
            "a prompt",
        ],
        Some(&stub),
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recorded = fs::read_to_string(&log).expect("read stub arguments");
    let lines: Vec<&str> = recorded.lines().collect();
    assert_eq!(
        lines,
        vec![
            "--headless",
            "--json",
            "--model",
            "capture",
            "--effort",
            "high",
            "a prompt"
        ]
    );
}

#[test]
fn prompt_headless_reads_redirected_standard_input() {
    let project = configured_project();
    let log = project.path().join("stub-args.txt");
    let stub = install_stub_agent_runner(project.path(), &log);

    let mut child = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt"])
        .env("KVIST_AGENT_RUNNER", &stub)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start prompt command");
    child
        .stdin
        .take()
        .expect("prompt stdin")
        .write_all(b"Prompt piped through stdin")
        .expect("write prompt");

    let output = child.wait_with_output().expect("wait for prompt command");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recorded = fs::read_to_string(&log).expect("read stub arguments");
    assert!(recorded.contains("Prompt piped through stdin"));
}

#[test]
fn prompt_headless_can_be_authored_with_an_editor() {
    let project = configured_project();
    let log = project.path().join("stub-args.txt");
    let stub = install_stub_agent_runner(project.path(), &log);
    let editor = project.path().join("editor.sh");
    fs::write(
        &editor,
        "#!/bin/sh\nprintf 'Prompt authored in editor' > \"$1\"\n",
    )
    .expect("write editor");
    fs::set_permissions(&editor, fs::Permissions::from_mode(0o700))
        .expect("make editor executable");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "--editor"])
        .env("KVIST_AGENT_RUNNER", &stub)
        .env("VISUAL", &editor)
        .env_remove("EDITOR")
        .output()
        .expect("run prompt command");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let recorded = fs::read_to_string(&log).expect("read stub arguments");
    assert!(recorded.contains("Prompt authored in editor"));
}

#[test]
fn prompt_headless_rejects_host_execution() {
    let project = configured_project();

    let output = run_kvist_in(
        project.path(),
        &["prompt", "--allow-host-execution", "host"],
        None,
    );

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 error")
            .contains("interactive only")
    );
}

#[test]
fn prompt_requires_an_installed_agent_runner() {
    let project = configured_project();

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "a prompt"])
        .env("KVIST_AGENT_RUNNER", "")
        .env("PATH", "")
        .output()
        .expect("run prompt command");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 error")
            .contains("agent-runner")
    );
}

#[test]
fn prompt_headless_surfaces_child_failure() {
    let project = configured_project();
    let stub = project.path().join("failing-agent-runner");
    fs::write(&stub, "#!/bin/sh\nexit 3\n").expect("write failing stub");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o700)).expect("make stub executable");

    let output = run_kvist_in(project.path(), &["prompt", "a prompt"], Some(&stub));

    assert!(!output.status.success());
}

#[test]
fn prompt_rejects_conflicting_explicit_sources() {
    let project = configured_project();
    let prompt_path = project.path().join("prompt.md");
    fs::write(&prompt_path, "file prompt").expect("write prompt");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "positional prompt", "--file"])
        .arg(&prompt_path)
        .env_remove("KVIST_AGENT_RUNNER")
        .output()
        .expect("run conflicting prompt command");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 error")
            .contains("cannot be used with")
    );
}

#[test]
fn prompt_rejects_oversized_files_before_agent_execution() {
    let project = configured_project();
    let prompt_path = project.path().join("prompt.md");
    fs::write(&prompt_path, vec![b'x'; MAX_PROMPT_BYTES as usize + 1])
        .expect("write oversized prompt");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "--file"])
        .arg(&prompt_path)
        .env_remove("KVIST_AGENT_RUNNER")
        .output()
        .expect("run oversized prompt command");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 error")
            .contains("exceeds the 1048576-byte limit")
    );
}

#[test]
fn prompt_rejects_linked_files_before_agent_execution() {
    let project = configured_project();
    let target = project.path().join("target.md");
    let prompt_path = project.path().join("prompt.md");
    fs::write(&target, "linked prompt").expect("write prompt target");
    std::os::unix::fs::symlink(&target, &prompt_path).expect("link prompt");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "--file"])
        .arg(&prompt_path)
        .env_remove("KVIST_AGENT_RUNNER")
        .output()
        .expect("run linked prompt command");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 error")
            .contains("regular non-link file")
    );
}
