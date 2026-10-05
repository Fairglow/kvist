use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use kvist::init::initialize;
use tempfile::TempDir;

const GENERATED_REQUIREMENTS_REVISION: &str =
    "sha256:bd53663c2dc76fdcbe58b111c0174a7550a3e3fe773a1c3e4a14196c1089dfa0";

fn run_kvist(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .output()
        .expect("run kvist command")
}

fn valid_queue(requirements_revision: &str, tasks: &str) -> String {
    format!(
        "schema_version: 1\ncomponent:\n  requirements_revision: {requirements_revision}\n  contract_revision: sha256:54b07fd8cbfb911f7e8546854b49944eb499429ea14cf302c2cba0f64238b98c\n  design_revision: sha256:6d6579ce1b018dce3b34e87afd72b494d27692ada8d54003b0a10ecd74abed17\n  parent_contract: null\n  revalidation:\n    state: current\n    checked_at: 2026-08-16T12:19:23Z\n    stale_since: null\n    causes: []\ntasks:{tasks}\n"
    )
}

fn unsorted_queue() -> String {
    valid_queue(
        GENERATED_REQUIREMENTS_REVISION,
        r#"
  - id: investigate
    title: Investigate repair fixture
    description: Preserve an unsorted requirement list for repair testing.
    context: Repair must canonicalize the queue.
    purpose: Verify the defined set-list rewrite.
    expected_outcome: The queue is sorted and duplicate-free.
    kind: test
    status: pending
    depends_on: []
    requirements:
      - REQUIREMENTS.md#Project-status-inspection
      - CONTRACT.md#Provided-interfaces
    timestamps:
      created_at: 2026-08-16T12:19:23Z
      updated_at: 2026-08-16T12:19:23Z
      completed_at: null
    blocked_reason: null
    recovery_state: null
"#,
    )
}

fn initialized_with(path: &Path, queue: &str) {
    initialize(path).expect("initialize");
    fs::write(path.join("src/TODOS.yaml"), queue).expect("write queue");
}

#[test]
fn repair_canonicalizes_an_unsorted_queue_and_recovers_the_project() {
    let project = TempDir::new().expect("project");
    let queue_path = project.path().join("src/TODOS.yaml");
    initialized_with(project.path(), &unsorted_queue());
    let before = fs::read_to_string(&queue_path).expect("read queue");

    let output = run_kvist(&[
        "repair",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(
        output.status.success(),
        "{:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 repair output");
    assert!(stdout.contains("state: invalid -> current"));
    assert!(stdout.contains("src/TODOS.yaml: repaired"));
    assert!(stdout.contains("every root artifact is valid"));
    assert_ne!(
        fs::read_to_string(&queue_path).expect("re-read queue"),
        before
    );

    // The rewritten queue is canonical: a second repair changes nothing.
    let canonical = fs::read_to_string(&queue_path).expect("read canonical queue");
    let second = run_kvist(&[
        "repair",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(second.status.success());
    let stdout = String::from_utf8(second.stdout).expect("UTF-8 second repair output");
    assert!(stdout.contains("state: current -> current"));
    assert!(stdout.contains("src/TODOS.yaml: unchanged"));
    assert_eq!(
        fs::read_to_string(&queue_path).expect("re-read canonical queue"),
        canonical
    );
}

#[test]
fn repair_dry_run_reports_without_writing() {
    let project = TempDir::new().expect("project");
    let queue_path = project.path().join("src/TODOS.yaml");
    initialized_with(project.path(), &unsorted_queue());
    let before = fs::read(&queue_path).expect("read queue");

    let output = run_kvist(&[
        "repair",
        "--dry-run",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(
        !output.status.success(),
        "dry run must fail while the defect remains"
    );
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 repair stderr");
    assert!(stderr.contains("dry-run: true"));
    assert!(stderr.contains("src/TODOS.yaml: would-repair"));
    assert!(stderr.contains("must be lexically sorted and duplicate-free"));
    assert!(stderr.contains("still need manual repair"));
    assert_eq!(fs::read(&queue_path).expect("re-read queue"), before);
}

#[test]
fn repair_leaves_a_valid_noncanonical_queue_untouched() {
    let project = TempDir::new().expect("project");
    let queue_path = project.path().join("src/TODOS.yaml");
    // Valid (parses) but not byte-canonical: flow lists and unquoted scalars.
    let noncanonical = valid_queue(
        GENERATED_REQUIREMENTS_REVISION,
        r#"
  - id: investigate
    title: Investigate repair fixture
    description: Preserve a valid non-canonical queue for repair testing.
    context: Repair must not rewrite queues that already parse.
    purpose: Keep repairs surgical.
    expected_outcome: The queue is reported and left untouched.
    kind: test
    status: pending
    depends_on: []
    requirements: ["CONTRACT.md#Provided-interfaces"]
    timestamps:
      created_at: 2026-08-16T12:19:23Z
      updated_at: 2026-08-16T12:19:23Z
      completed_at: null
    blocked_reason: null
    recovery_state: null
"#,
    );
    initialized_with(project.path(), &noncanonical);
    let before = fs::read(&queue_path).expect("read queue");

    let output = run_kvist(&[
        "repair",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 repair output");
    assert!(stdout.contains("state: current -> current"));
    assert!(stdout.contains("src/TODOS.yaml: non-canonical"));
    assert_eq!(fs::read(&queue_path).expect("re-read queue"), before);
}

#[test]
fn repair_refuses_a_fenced_queue() {
    let project = TempDir::new().expect("project");
    let queue_path = project.path().join("src/TODOS.yaml");
    let fenced = valid_queue(
        GENERATED_REQUIREMENTS_REVISION,
        r#"
  - id: seed
    title: Seed test task
    description: A completed test predecessor.
    context: Fixture for fenced repair.
    purpose: Keep the lifecycle chain complete.
    expected_outcome: The implementation task has a completed test predecessor.
    kind: test
    status: completed
    depends_on: []
    requirements:
      - REQUIREMENTS.md#Project-status-inspection
    timestamps:
      created_at: 2026-08-16T12:19:23Z
      updated_at: 2026-08-16T12:19:23Z
      completed_at: 2026-08-16T12:19:23Z
    blocked_reason: null
    recovery_state: null
  - id: fenced-task
    title: Fenced implementation task
    description: An in-progress task with a fenced attempt and unsorted requirements.
    context: Repair must not rewrite a fenced queue.
    purpose: Keep recovery authority exclusive to kvist task recover.
    expected_outcome: The queue is reported fenced and left untouched.
    kind: implementation
    status: in-progress
    depends_on:
      - seed
    requirements:
      - REQUIREMENTS.md#Project-status-inspection
      - CONTRACT.md#Provided-interfaces
    timestamps:
      created_at: 2026-08-16T12:19:23Z
      updated_at: 2026-08-16T12:19:23Z
      completed_at: null
    blocked_reason: null
    recovery_state:
      state: fenced
      attempt_id: attempt-0001
      reason: runner descriptor failed before launch
"#,
    );
    initialized_with(project.path(), &fenced);
    let before = fs::read(&queue_path).expect("read queue");

    let output = run_kvist(&[
        "repair",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(
        !output.status.success(),
        "fenced defect remains; repair must fail"
    );
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 repair stderr");
    assert!(stderr.contains("src/TODOS.yaml: fenced"));
    assert!(stderr.contains("kvist task recover"));
    assert_eq!(fs::read(&queue_path).expect("re-read queue"), before);
}

#[test]
fn repair_reports_an_unparseable_queue_without_guessing() {
    let project = TempDir::new().expect("project");
    let queue_path = project.path().join("src/TODOS.yaml");
    initialized_with(project.path(), "schema_version: 1\ntasks: [broken\n");
    let before = fs::read(&queue_path).expect("read queue");

    let output = run_kvist(&[
        "repair",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(
        !output.status.success(),
        "unparseable queue cannot be repaired"
    );
    let stderr = String::from_utf8(output.stderr).expect("UTF-8 repair stderr");
    assert!(stderr.contains("src/TODOS.yaml: unparseable"));
    assert_eq!(fs::read(&queue_path).expect("re-read queue"), before);
}
