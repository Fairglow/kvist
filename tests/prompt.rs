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
        .args(["prompt", "--file"])
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
        .arg("prompt")
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
        .args(["prompt", "--editor"])
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
        .args(["prompt", "--file"])
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
        .args(["prompt", "--file"])
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
