use std::process::Command;

use sav::observation::RunJournal;
use serde_json::json;

fn skott() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_skott"));
    command.env_clear().env("PATH", "/usr/bin:/bin");
    command
}

#[test]
fn attachment_needs_no_configuration_and_marks_incomplete_snapshot() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("run.jsonl");
    let mut journal =
        RunJournal::create(&path, "fixture", json!({"host":"kvist"})).expect("journal");
    journal
        .record(&json!({"type":"note","data":"visible\u{1b}[2Jtext"}))
        .expect("note");
    let output = skott()
        .arg("--attach")
        .arg(&path)
        .output()
        .expect("snapshot");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("utf8");
    assert!(text.contains("visible"));
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("incomplete"));
    journal.finish("completed").expect("finish");
    let output = skott()
        .arg("--attach")
        .arg(path)
        .output()
        .expect("complete snapshot");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("completed"));
}

#[test]
fn malformed_journals_fail_and_control_requires_explicit_capability() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("invalid");
    std::fs::write(&path, b"invalid\n").expect("fixture");
    let output = skott()
        .arg("--attach")
        .arg(&path)
        .output()
        .expect("invalid");
    assert!(!output.status.success());
    let output = skott()
        .arg("--attach")
        .arg(&path)
        .arg("--cancel-run")
        .output()
        .expect("parser");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--control"));
}

#[test]
fn explicit_control_reports_queued_not_delivered() {
    use sav::observation::{ControlReply, ControlServer};
    let server = ControlServer::bind(|request| ControlReply::queued(request.id)).expect("server");
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("run");
    let mut journal = RunJournal::create(&path, "fixture", json!({})).expect("journal");
    journal.finish("completed").expect("finish");
    let output = skott()
        .arg("--attach")
        .arg(path)
        .arg("--control")
        .arg(server.path())
        .arg("--guide")
        .arg("retain original goal")
        .output()
        .expect("control");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("queued"));
}
