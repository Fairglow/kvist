use std::{
    fs,
    process::{Command, Output},
};

use kvist::init::initialize;
use tempfile::TempDir;

const GENERATED_SPECIFICATION_REVISION: &str =
    "sha256:d47faba18fc80961e3cf1872cbd0d74ccc114a9667dfbc6b84dbbfac2234a1bd";

fn run_kvist(project: &TempDir, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .current_dir(project.path())
        .output()
        .expect("run kvist command")
}

fn track_project(project: &TempDir) {
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(project.path())
        .status()
        .expect("initialize Git");
    assert!(status.success());
    let status = Command::new("git")
        .args(["add", "."])
        .current_dir(project.path())
        .status()
        .expect("track project artifacts");
    assert!(status.success());
}

fn queue() -> String {
    format!(
        r#"schema_version: 2
component:
  specification_revision: {GENERATED_SPECIFICATION_REVISION}
  parent_specification: null
  revalidation:
    state: current
    checked_at: 2026-08-16T12:54:50Z
    stale_since: null
    causes: []
tasks:
  - id: write-tests
    title: Write tests
    description: Define the task-command contract.
    context: Selection needs a completed test predecessor.
    purpose: Preserve lifecycle order.
    expected_outcome: The test stage is complete.
    kind: test
    status: completed
    depends_on: []
    requirements:
      - SPEC.md#Task-selection-and-state-updates
    timestamps:
      created_at: 2026-08-16T12:54:50Z
      updated_at: 2026-08-16T12:54:50Z
      completed_at: 2026-08-16T12:54:50Z
    blocked_reason: null
  - id: implement-code
    title: Implement task command
    description: Persist one legal transition.
    context: The next task must be selected deterministically.
    purpose: Exercise durable workflow updates.
    expected_outcome: The task becomes active with audit evidence.
    kind: implementation
    status: pending
    depends_on:
      - write-tests
    requirements:
      - SPEC.md#Task-selection-and-state-updates
    timestamps:
      created_at: 2026-08-16T12:54:50Z
      updated_at: 2026-08-16T12:54:50Z
      completed_at: null
    blocked_reason: null
"#
    )
}

#[test]
fn task_next_selects_the_first_ready_task_without_writing_the_queue() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    let queue_path = project.path().join("src/TODOS.yaml");
    fs::write(&queue_path, queue()).expect("write queue");
    track_project(&project);
    let before = fs::read(&queue_path).expect("read before");

    let output = run_kvist(&project, &["task", "next", "."]);

    assert!(output.status.success());
    assert_eq!(output.stderr, b"");
    assert_eq!(output.stdout, b"implement-code\n");
    assert_eq!(fs::read(&queue_path).expect("read after"), before);
}

#[test]
fn task_next_requires_complete_vcs_tracking() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    let output = run_kvist(&project, &["task", "next", "."]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("VCS tracked"));
}

#[test]
fn task_transition_writes_auditable_atomic_state_change() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    let queue_path = project.path().join("src/TODOS.yaml");
    fs::write(&queue_path, queue()).expect("write queue");
    track_project(&project);

    let output = run_kvist(
        &project,
        &["task", "transition", ".", "implement-code", "in-progress"],
    );

    assert!(output.status.success());
    assert_eq!(output.stderr, b"");
    assert_eq!(
        output.stdout,
        b"transitioned implement-code to in-progress\n"
    );
    let queue = fs::read_to_string(&queue_path).expect("read updated queue");
    assert!(queue.contains("id: \"implement-code\""));
    assert!(queue.contains("status: in-progress"));
    let audit = fs::read_to_string(
        project
            .path()
            .join("src/.kvist-attempts/implement-code.jsonl"),
    )
    .expect("read audit");
    assert!(audit.contains("\"prepared\""));
    assert!(audit.contains("\"committed\""));
    assert!(!project.path().join("src/.kvist-task.lock").exists());
}

#[test]
fn task_transition_refuses_an_existing_component_lock() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    track_project(&project);
    fs::write(
        project.path().join("src/.kvist-task.lock"),
        "started_at: 2026-08-16T12:54:50Z\ntask_id: implement-code\n",
    )
    .expect("write retained lock");

    let output = run_kvist(
        &project,
        &["task", "transition", ".", "implement-code", "in-progress"],
    );

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("lock"));
    assert!(
        !project
            .path()
            .join("src/.kvist-attempts/implement-code.jsonl")
            .exists()
    );
}

#[test]
fn task_transition_requires_a_block_reason() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    track_project(&project);

    let output = run_kvist(
        &project,
        &["task", "transition", ".", "implement-code", "blocked"],
    );

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--reason"));
    assert!(!project.path().join("src/.kvist-task.lock").exists());
}
