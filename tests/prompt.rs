#![cfg(unix)]

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
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

#[test]
fn prompt_reads_text_from_a_file() {
    let project = configured_project();
    let prompt_path = project.path().join("prompt.md");
    fs::write(&prompt_path, "Prompt loaded from a file").expect("write prompt");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "--allow-host-execution", "--file"])
        .arg(&prompt_path)
        .output()
        .expect("run prompt command");

    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 output")
            .contains("Prompt loaded from a file")
    );
}

#[test]
fn prompt_reads_redirected_standard_input() {
    let project = configured_project();
    let mut child = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "--allow-host-execution"])
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
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 output")
            .contains("Prompt piped through stdin")
    );
}

#[test]
fn prompt_can_be_authored_with_an_editor() {
    let project = configured_project();
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
        .args(["prompt", "--allow-host-execution", "--editor"])
        .env("VISUAL", &editor)
        .env_remove("EDITOR")
        .output()
        .expect("run prompt command");

    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .expect("UTF-8 output")
            .contains("Prompt authored in editor")
    );
}

#[test]
fn prompt_rejects_conflicting_explicit_sources() {
    let project = configured_project();
    let prompt_path = project.path().join("prompt.md");
    fs::write(&prompt_path, "file prompt").expect("write prompt");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "positional prompt", "--file"])
        .arg(prompt_path)
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
        .args(["prompt", "--allow-host-execution", "--file"])
        .arg(prompt_path)
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
        .args(["prompt", "--allow-host-execution", "--file"])
        .arg(prompt_path)
        .output()
        .expect("run linked prompt command");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 error")
            .contains("regular non-link file")
    );
}

#[test]
fn prompt_requires_explicit_host_execution_acknowledgement() {
    let project = configured_project();

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "host prompt"])
        .output()
        .expect("run prompt command");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 error")
            .contains("--allow-host-execution")
    );
}

#[test]
fn prompt_text_output_has_no_synthetic_completion_trailer() {
    let project = configured_project();

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "--allow-host-execution", "answer"])
        .output()
        .expect("run prompt command");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        "answer\n"
    );
}

#[test]
fn prompt_json_captures_one_selected_model_result() {
    let project = TempDir::new().expect("create project");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"

[agent.profiles.developer]
model = "first"
default_model = "first"
models = [
  { name = "first", command = "/bin/echo first" },
  { name = "selected", command = "/bin/echo '{reasoning_effort}'" },
]
"#,
    )
    .expect("write configuration");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args([
            "--json",
            "prompt",
            "--allow-host-execution",
            "--model",
            "selected",
            "--reasoning-effort",
            "high",
            "ignored",
        ])
        .output()
        .expect("run JSON prompt");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("single JSON output"),
        serde_json::json!({"content": "high\n"})
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn prompt_json_replaces_invalid_utf8_provider_bytes() {
    let project = TempDir::new().expect("create project");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"

[agent.profiles.developer]
model = "bytes"
default_model = "bytes"
models = [{ name = "bytes", command = "sh -c 'printf \"\\377\"'" }]
"#,
    )
    .expect("write configuration");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["--json", "prompt", "--allow-host-execution", "ignored"])
        .output()
        .expect("run JSON prompt with invalid UTF-8");

    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("single JSON output"),
        serde_json::json!({"content": "\u{fffd}"})
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn prompt_reasoning_effort_requires_selected_command_support() {
    let project = configured_project();

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args([
            "prompt",
            "--allow-host-execution",
            "--reasoning-effort",
            "high",
            "ignored",
        ])
        .output()
        .expect("run unsupported reasoning effort");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("{reasoning_effort}")
    );
}

#[test]
fn prompt_accepts_every_typed_reasoning_effort() {
    let project = TempDir::new().expect("create project");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"

[agent.profiles.developer]
model = "effort"
default_model = "effort"
models = [{ name = "effort", command = "/bin/echo '{reasoning_effort}'" }]
"#,
    )
    .expect("write configuration");

    for effort in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
        let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
            .current_dir(project.path())
            .args([
                "prompt",
                "--allow-host-execution",
                "--reasoning-effort",
                effort,
                "ignored",
            ])
            .output()
            .expect("run typed reasoning effort");

        assert!(
            output.status.success(),
            "{effort}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("{effort}\n")
        );
    }
}

#[test]
fn prompt_passes_shell_metacharacters_as_literal_content() {
    let project = TempDir::new().expect("create project");
    let marker = project.path().join("must-not-exist");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"

[agent.profiles.developer]
model = "literal"
default_model = "literal"
models = [{ name = "literal", command = "/usr/bin/printf '%s' '{prompt}'" }]
"#,
    )
    .expect("write configuration");
    let prompt = format!("$(touch {}) ; echo injected", marker.display());

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "--allow-host-execution", &prompt])
        .output()
        .expect("run literal shell metacharacters");

    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), prompt);
    assert!(!marker.exists());
}

#[test]
fn prompt_enforces_the_configured_output_bound() {
    let project = TempDir::new().expect("create project");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"

[agent.profiles.developer]
model = "bounded"
default_model = "bounded"
max_output_bytes = 4
models = [{ name = "bounded", command = "/usr/bin/printf 123456789" }]
"#,
    )
    .expect("write configuration");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .current_dir(project.path())
        .args(["prompt", "--allow-host-execution", "ignored"])
        .output()
        .expect("run bounded provider");

    assert!(!output.status.success());
    assert!(output.stdout.len() <= 4);
    assert!(
        String::from_utf8(output.stderr)
            .expect("UTF-8 stderr")
            .contains("output limit")
    );
}
