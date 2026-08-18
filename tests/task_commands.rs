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

#[test]
fn spec_accept_resolves_staleness_and_updates_queue_revisions() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    track_project(&project);

    // 1. Modify the specification slightly to make it stale
    let spec_path = project.path().join("src/SPEC.md");
    let original_spec = fs::read_to_string(&spec_path).expect("read spec");
    // Append a minor valid edit inside Layer 1 without breaking structural headings
    let updated_spec =
        original_spec.replace("## Purpose", "## Purpose\nThis is a newly accepted change.");
    fs::write(&spec_path, &updated_spec).expect("write updated spec");

    // Verify it is stale under status
    let output = run_kvist(&project, &["status", "."]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("state: stale"));

    // 2. Run spec accept
    let output = run_kvist(&project, &["spec", "accept", "."]);
    assert!(output.status.success());
    assert_eq!(output.stderr, b"");
    assert!(String::from_utf8_lossy(&output.stdout).contains("accepted specification change"));

    // Verify it is no longer stale under status
    let output = run_kvist(&project, &["status", "."]);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("state: stale"));

    // Verify TODOS.yaml has been updated with the correct SHA-256 hash
    use sha2::{Digest, Sha256};
    let expected_hash = format!("sha256:{:x}", Sha256::digest(updated_spec.as_bytes()));
    let queue_contents =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue_contents.contains(&expected_hash));
    assert!(queue_contents.contains("state: current"));
}

#[test]
fn spec_accept_rejects_invalid_specifications() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // 1. Create a valid child component
    let child_dir = project.path().join("src/child");
    fs::create_dir_all(&child_dir).expect("create child dir");
    let output_child_spec = run_kvist(&project, &["spec", "new", "src/child"]);
    assert!(output_child_spec.status.success());
    fs::write(
        child_dir.join("DOCS.md"),
        "<!-- kvist-documentation-version: 1 -->\n# Root Component Compliance Documentation\n",
    )
    .expect("write child docs");

    // Create a child TODOS.yaml with parent reference
    let child_queue = format!(
        r#"schema_version: 2
component:
  specification_revision: sha256:d47faba18fc80961e3cf1872cbd0d74ccc114a9667dfbc6b84dbbfac2234a1bd
  parent_specification:
    path: ../SPEC.md
    revision: {GENERATED_SPECIFICATION_REVISION}
  revalidation:
    state: current
    checked_at: 2026-08-16T12:54:50Z
    stale_since: null
    causes: []
tasks: []
"#
    );
    fs::write(child_dir.join("TODOS.yaml"), &child_queue).expect("write child queue");
    track_project(&project);

    // 2. Write an invalid child spec
    let spec_path = child_dir.join("SPEC.md");
    fs::write(&spec_path, "# invalid specification").expect("write invalid child spec");

    // 3. Run spec accept and assert failure
    let output = run_kvist(&project, &["spec", "accept", "child"]);
    assert!(
        !output.status.success(),
        "Expected failure but got success. Stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("is invalid"),
        "Expected 'is invalid' in stderr, but got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!project.path().join("src/child/.kvist-task.lock").exists());
}

#[test]
fn spec_accept_on_child_updates_parent_revision() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // 1. Initialize a child component at src/child
    let child_dir = project.path().join("src/child");
    fs::create_dir_all(&child_dir).expect("create child dir");
    let output_child = run_kvist(&project, &["spec", "new", "src/child"]);
    assert!(
        output_child.status.success(),
        "Failed to create child spec. Stderr: {}",
        String::from_utf8_lossy(&output_child.stderr)
    );
    fs::write(
        child_dir.join("DOCS.md"),
        "<!-- kvist-documentation-version: 1 -->\n# Root Component Compliance Documentation\n",
    )
    .expect("write child docs");

    // Create a child TODOS.yaml with parent reference
    let child_queue = format!(
        r#"schema_version: 2
component:
  specification_revision: sha256:d47faba18fc80961e3cf1872cbd0d74ccc114a9667dfbc6b84dbbfac2234a1bd
  parent_specification:
    path: ../SPEC.md
    revision: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  revalidation:
    state: stale
    checked_at: 2026-08-16T12:54:50Z
    stale_since: 2026-08-16T12:54:50Z
    causes:
      - kind: parent-specification-revision-changed
        path: ../SPEC.md
        expected_revision: sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
        observed_revision: sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
tasks: []
"#
    );
    fs::write(child_dir.join("TODOS.yaml"), &child_queue).expect("write child queue");
    track_project(&project);

    // Let's run status and print it out to see why child is considered invalid!
    let status_out = run_kvist(&project, &["status", "."]);
    println!(
        "STATUS OUTPUT:\n{}",
        String::from_utf8_lossy(&status_out.stdout)
    );
    println!(
        "STATUS STDERR:\n{}",
        String::from_utf8_lossy(&status_out.stderr)
    );

    // 2. Run spec accept on child
    let output = run_kvist(&project, &["spec", "accept", "child"]);
    assert!(
        output.status.success(),
        "spec accept child failed. Stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("accepted specification change"));

    // Verify child TODOS.yaml has been updated with parent's correct SHA-256 hash
    let parent_spec_contents =
        fs::read_to_string(project.path().join("src/SPEC.md")).expect("read parent spec");
    use sha2::{Digest, Sha256};
    let parent_hash = format!(
        "sha256:{:x}",
        Sha256::digest(parent_spec_contents.as_bytes())
    );

    let updated_child_contents =
        fs::read_to_string(child_dir.join("TODOS.yaml")).expect("read child queue");
    assert!(updated_child_contents.contains(&parent_hash));
    assert!(updated_child_contents.contains("state: current"));
}
