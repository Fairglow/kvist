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
    configure_fake_sandbox(project);
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

#[cfg(target_os = "linux")]
fn fake_sandbox_runner_path(project: &TempDir) -> std::path::PathBuf {
    project
        .path()
        .parent()
        .expect("temporary project parent")
        .join(format!(
            "fake-sandbox-runner-{}",
            project
                .path()
                .file_name()
                .expect("temporary project name")
                .to_string_lossy()
        ))
}

#[cfg(target_os = "linux")]
fn configure_fake_sandbox(project: &TempDir) {
    use std::os::unix::fs::PermissionsExt;

    let runner = fake_sandbox_runner_path(project);
    fs::write(
        &runner,
        r#"#!/bin/sh
if [ "$1" = "--kvist-sandbox-probe-v1" ]; then
  printf 'kvist-sandbox-probe-v1: network=deny; mount=component\n'
  exit 0
fi
request=$(cat)
printf '%s' "$request" > sandbox-request.json
case "$request" in
  *'"program":"false"'*) exit 1 ;;
  *'agent-overflow'*)
    i=0
    while [ "$i" -lt 10000 ]; do
      printf 'secret-agent-output'
      printf 'secret-agent-output' >&2
      i=$((i + 1))
    done
    ;;
  *'agent-sleep'*) sleep 2 ;;
  *'delete-lock'*)
    rm -f src/.kvist-task.lock
    sleep 2
    ;;
  *'split-secret'*)
    printf 'cross-stream-'
    printf 'secret' >&2
    exit 0
    ;;
  *'verification-secret'*)
    printf 'agent-redaction-secret'
    printf '%s' "$KVIST_TEST_SECRET" >&2
    exit 1
    ;;
  *'emit-and-fail'*|*'fail-test'*)
    i=0
    while [ "$i" -lt 10000 ]; do
      printf 1234567890abcdef
      i=$((i + 1))
    done
    exit 1
    ;;
  *'sleep'*) sleep 2 ;;
esac
printf 'fake sandbox runner\n'
"#,
    )
    .expect("write fake sandbox runner");
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
        .expect("make runner executable");
    let config_path = project.path().join("kvist.toml");
    let config = fs::read_to_string(&config_path).expect("read config");
    if !config.contains("[sandbox]") {
        fs::write(
            config_path,
            format!(
                "{config}\n[sandbox]\nschema_version = 1\nrunner = \"{}\"\nnetwork = \"deny\"\nenvironment_allowlist = []\nmount = \"component\"\n",
                runner.display()
            ),
        )
        .expect("configure sandbox");
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_fake_sandbox(_project: &TempDir) {}

fn queue() -> String {
    format!(
        r#"schema_version: 1
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
fn task_transition_ignores_an_agent_visible_component_lock() {
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

    assert!(output.status.success());
    assert!(
        project
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
        child_dir.join("IMPL.md"),
        "<!-- kvist-implementation-record-version: 1 -->\n# Root Component Implementation Record\n",
    )
    .expect("write child docs");

    // Create a child TODOS.yaml with parent reference
    let child_queue = format!(
        r#"schema_version: 1
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
        child_dir.join("IMPL.md"),
        "<!-- kvist-implementation-record-version: 1 -->\n# Root Component Implementation Record\n",
    )
    .expect("write child docs");

    // Create a child TODOS.yaml with parent reference
    let child_queue = r#"schema_version: 1
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
    .to_string();
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

#[test]
#[cfg(target_os = "linux")]
fn task_run_executes_successfully_and_transitions_completed() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // Configure a mock echo command in kvist.toml for developer agent profile and test policy
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo 'mocking execute' {context_files}"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo 'mocking verify'"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);

    // Approve test policy
    let approve_output = run_kvist(&project, &["task", "approve-policy"]);
    assert!(
        approve_output.status.success(),
        "{}",
        String::from_utf8_lossy(&approve_output.stderr)
    );

    // Run next task (implement-code)
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(
        output.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stderr, b"");
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout_str.contains("executed and verified successfully and transitioned to completed")
    );
    assert!(stdout_str.contains("Logs written to"));

    // Verify task status is indeed Completed in TODOS.yaml
    let queue_contents =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue_contents.contains("status: completed"));
    let manifest = fs::read_to_string(project.path().join("sandbox-request.json"))
        .expect("read sandbox manifest");
    assert!(manifest.contains("\"protocol_version\":1"));
    assert!(manifest.contains("\"network\":\"deny\""));
    assert!(manifest.contains("\"destination\":\"/workspace/component\""));
    assert!(manifest.contains("\"working_directory\":\"/workspace/component\""));
}

#[test]
#[cfg(target_os = "linux")]
fn approval_record_is_deterministic_and_rejects_changed_agent_template_before_probe() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo approved-agent"
token_limit = 42
[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = []
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo verify"
"#,
    )
    .expect("write config");
    track_project(&project);

    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );
    let first = run_kvist(&project, &["task", "approve-policy"]);
    assert!(first.status.success());
    let second = run_kvist(&project, &["task", "approve-policy"]);
    assert!(second.status.success());
    assert_eq!(first.stdout, second.stdout);

    let changed = fs::read_to_string(project.path().join("kvist.toml"))
        .expect("read config")
        .replace("approved-agent", "substituted-agent");
    fs::write(project.path().join("kvist.toml"), changed).expect("change template");
    Command::new("git")
        .args(["add", "kvist.toml"])
        .current_dir(project.path())
        .status()
        .expect("stage changed config");
    let before = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");

    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("agent configuration source"));
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue"),
        before
    );
    assert!(!project.path().join("sandbox-request.json").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn approval_rejects_changed_agent_source_and_runner_content_before_mutation() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    fs::create_dir_all(project.path().join(".kvist")).expect("create local config directory");
    fs::write(
        project.path().join(".kvist/config.toml"),
        "[agent.profiles.developer]\ncommand_template = \"echo local-agent\"\ntimeout_seconds = 5\nmax_output_bytes = 1000\n",
    )
    .expect("write local agent config");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"
[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = []
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo verify"
"#,
    )
    .expect("write project config");
    track_project(&project);
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );

    fs::write(
        project.path().join(".kvist/config.toml"),
        "[agent.profiles.developer]\ncommand_template = \"echo changed-local-agent\"\n",
    )
    .expect("change local agent config");
    let before = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("agent configuration source"));
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue"),
        before
    );

    fs::write(
        project.path().join(".kvist/config.toml"),
        "[agent.profiles.developer]\ncommand_template = \"echo local-agent\"\ntimeout_seconds = 5\nmax_output_bytes = 1000\n",
    )
    .expect("restore local agent config");
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );
    fs::write(fake_sandbox_runner_path(&project), "#!/bin/sh\nexit 0\n").expect("change runner");

    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sandbox runner"));
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue"),
        before
    );
}

#[test]
#[cfg(target_os = "linux")]
fn approval_records_absent_test_policy_but_task_run_refuses_it_before_mutation() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    let config = "schema_version = 1\ncomponent_root = \"src\"\n[agent.profiles.developer]\ncommand_template = \"echo agent\"\n";
    fs::write(project.path().join("kvist.toml"), config).expect("write config");
    track_project(&project);

    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );
    let before = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("test policy is absent"));
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue"),
        before
    );
}

#[test]
#[cfg(target_os = "linux")]
fn repository_forged_approval_record_cannot_probe_or_execute() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo forged"
[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = []
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo verify"
"#,
    )
    .expect("write forged config");
    track_project(&project);
    fs::create_dir_all(project.path().join(".kvist")).expect("create legacy record directory");
    fs::write(
        project.path().join(".kvist/approved_execution_policy.json"),
        r#"{
  "schema_version": 1,
  "canonical_project": "/forged/project",
  "canonical_worktree": "/forged/worktree",
  "material": {
    "configuration_schema_version": 1,
    "approval_schema_version": 1,
    "sandbox_protocol_version": 1,
    "sandbox_schema_version": 1,
    "agent_source": "/forged/config.toml",
    "agent_source_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    "architect_template_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    "architect_token_limit": null,
    "developer_template_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    "developer_token_limit": null,
    "sandbox_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    "runner_path": "/forged/runner",
    "runner_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    "test_policy_schema_version": 1,
    "test_policy_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
  },
  "approval_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
  "authentication_tag": "hmac-sha256:forged"
}"#,
    )
    .expect("forge every project-controlled approval value");
    let before = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");

    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("repository-contained approval record")
    );
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue"),
        before
    );
    assert!(!project.path().join("sandbox-request.json").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn runner_changed_after_probe_is_not_spawned_for_request() {
    use std::os::unix::fs::PermissionsExt;

    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo agent"
[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = []
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo verify"
"#,
    )
    .expect("write config");
    let runner = fake_sandbox_runner_path(&project);
    fs::write(
        &runner,
        format!(
            r#"#!/bin/sh
if [ "$1" = "--kvist-sandbox-probe-v1" ]; then
  cat > "{}" <<'EOF'
#!/bin/sh
printf 'untrusted request runner\n' > sandbox-request.json
exit 1
EOF
  chmod 755 "{}"
  printf 'kvist-sandbox-probe-v1: network=deny; mount=component\n'
  exit 0
fi
printf 'approved request runner\n' > sandbox-request.json
"#,
            runner.display(),
            runner.display()
        ),
    )
    .expect("write self-mutating runner");
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
        .expect("make runner executable");
    let config_path = project.path().join("kvist.toml");
    let config = fs::read_to_string(&config_path).expect("read config");
    fs::write(
        config_path,
        format!(
            "{config}\n[sandbox]\nschema_version = 1\nrunner = \"{}\"\nnetwork = \"deny\"\nenvironment_allowlist = []\nmount = \"component\"\n",
            runner.display()
        ),
    )
    .expect("configure runner");
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
        .expect("track project");
    assert!(status.success());
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );

    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("runner identity or content changed"));
    assert!(!project.path().join("sandbox-request.json").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn approval_rejects_changed_agent_limit_before_sandbox_probe() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    fs::create_dir_all(project.path().join(".kvist")).expect("create local config directory");
    fs::write(
        project.path().join(".kvist/config.toml"),
        "[agent.profiles.developer]\ncommand_template = \"echo local-agent\"\ntimeout_seconds = 5\nmax_output_bytes = 1000\n",
    )
    .expect("write local agent config");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"
[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = []
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo verify"
"#,
    )
    .expect("write config");
    track_project(&project);
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );
    let changed = fs::read_to_string(project.path().join(".kvist/config.toml"))
        .expect("read config")
        .replace("timeout_seconds = 5", "timeout_seconds = 6");
    fs::write(project.path().join(".kvist/config.toml"), changed).expect("change agent limit");
    let before = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");

    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("agent configuration source identity or digest has changed")
    );
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue"),
        before
    );
    assert!(!project.path().join("sandbox-request.json").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_refuses_missing_sandbox_before_transition() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    let config = fs::read_to_string(project.path().join("kvist.toml")).expect("read config");
    fs::write(
        project.path().join("kvist.toml"),
        format!("{config}\n[agent.profiles.developer]\ncommand_template = \"echo agent\"\n"),
    )
    .expect("write config");
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
        .expect("track project");
    assert!(status.success());
    let before = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");

    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sandbox configuration is absent"));
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue"),
        before
    );
    assert!(!project.path().join("src/.kvist-task.lock").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_refuses_a_project_local_sandbox_runner_before_transition() {
    use std::os::unix::fs::PermissionsExt;

    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    let runner = project.path().join("fake-sandbox-runner");
    fs::write(
        &runner,
        "#!/bin/sh\nprintf 'kvist-sandbox-probe-v1: network=deny; mount=component\\n'\n",
    )
    .expect("write runner");
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
        .expect("make runner executable");
    fs::write(
        project.path().join("kvist.toml"),
        format!(
            "schema_version = 1\ncomponent_root = \"src\"\n[agent.profiles.developer]\ncommand_template = \"echo agent\"\n[sandbox]\nschema_version = 1\nrunner = \"{}\"\nnetwork = \"deny\"\nenvironment_allowlist = []\nmount = \"component\"\n",
            runner.display()
        ),
    )
    .expect("write config");
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
        .expect("track project");
    assert!(status.success());
    let before = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");

    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("approval record")
            || String::from_utf8_lossy(&output.stderr).contains("outside the project root")
    );
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue"),
        before
    );
    assert!(!project.path().join("src/.kvist-task.lock").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_refuses_a_sibling_runner_in_the_selected_worktree() {
    use std::os::unix::fs::PermissionsExt;

    let checkout = TempDir::new().expect("checkout");
    let project = checkout.path().join("nested-project");
    initialize(&project).expect("initialize nested project");
    fs::write(project.join("src/TODOS.yaml"), queue()).expect("write queue");
    let runner = checkout.path().join("sibling-sandbox-runner");
    fs::write(
        &runner,
        "#!/bin/sh\nprintf 'kvist-sandbox-probe-v1: network=deny; mount=component\\n'\n",
    )
    .expect("write sibling runner");
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
        .expect("make runner executable");
    fs::write(
        project.join("kvist.toml"),
        format!(
            "schema_version = 1\ncomponent_root = \"src\"\n[agent.profiles.developer]\ncommand_template = \"echo agent\"\n[sandbox]\nschema_version = 1\nrunner = \"{}\"\nnetwork = \"deny\"\nenvironment_allowlist = []\nmount = \"component\"\n",
            runner.display()
        ),
    )
    .expect("write config");
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(checkout.path())
        .status()
        .expect("initialize checkout Git");
    assert!(status.success());
    let status = Command::new("git")
        .args(["add", "."])
        .current_dir(checkout.path())
        .status()
        .expect("track checkout");
    assert!(status.success());
    let before = fs::read_to_string(project.join("src/TODOS.yaml")).expect("read queue");

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(["task", "run", ".", "implement-code"])
        .current_dir(&project)
        .output()
        .expect("run kvist");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("approval record")
            || String::from_utf8_lossy(&output.stderr).contains("selected VCS worktree")
    );
    assert_eq!(
        fs::read_to_string(project.join("src/TODOS.yaml")).expect("read queue"),
        before
    );
    assert!(!project.join("src/.kvist-task.lock").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_auto_selects_and_executes_next_ready_task() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo 'mock auto select'"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo 'mocking verify'"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);

    // Approve test policy
    let approve_output = run_kvist(&project, &["task", "approve-policy"]);
    assert!(approve_output.status.success());

    // Omit task id argument; should auto-select implement-code
    let output = run_kvist(&project, &["task", "run", "."]);
    assert!(
        output.status.success(),
        "auto-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    assert!(stdout_str.contains("task `implement-code` executed and verified successfully"));
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_transitions_to_blocked_on_agent_failure() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // Configure a command template that exits with a failure code (e.g. false)
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "false"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = []
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo verify"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );

    // Run task and verify failure
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(output.status.success()); // Kvist CLI handles failures gracefully and exits 0 but marks task Blocked
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    assert!(stdout_str.contains("failed during execution and has been transitioned to blocked"));

    // Verify task status is Blocked in TODOS.yaml and contains the blocker_reason
    let queue_contents =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue_contents.contains("status: blocked"));
    assert!(queue_contents.contains("agent failed during task execution"));
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_cancels_agent_output_and_persists_redacted_bounded_evidence() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "agent-overflow"
timeout_seconds = 5
max_output_bytes = 64
[agent.profiles.developer.redaction]
values = ["secret-agent-output"]

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = []
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo verify"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );

    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("output limit"));
    let queue_contents =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue_contents.contains("status: blocked"));
    assert!(queue_contents.contains("combined output limit"));
    assert!(!queue_contents.contains("secret-agent-output"));
    let logs = fs::read_dir(project.path().join("src/.kvist/logs"))
        .expect("read logs")
        .next()
        .expect("log")
        .expect("log entry")
        .path();
    let log_contents = fs::read_to_string(logs).expect("read log");
    assert!(log_contents.len() <= 64);
    assert!(!log_contents.contains("secret-agent-output"));
    let attempts = fs::read_to_string(
        project
            .path()
            .join("src/.kvist-attempts/implement-code.jsonl"),
    )
    .expect("read attempts");
    assert!(attempts.contains("combined output limit"));
    assert!(!attempts.contains("secret-agent-output"));
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_cancels_agent_timeout_with_durable_evidence() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "agent-sleep"
timeout_seconds = 1
max_output_bytes = 1024

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = []
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo verify"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );

    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("timed out"));
    let queue_contents =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue_contents.contains("agent execution timed out"));
    assert!(
        project
            .path()
            .join("src/.kvist-attempts/implement-code.jsonl")
            .exists()
    );
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_redacts_a_secret_split_across_streams_before_log_and_streaming() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "split-secret"
timeout_seconds = 5
max_output_bytes = 1024
[agent.profiles.developer.redaction]
values = ["cross-stream-secret"]

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = []
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo verify"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );

    let output = run_kvist(
        &project,
        &["task", "run", ".", "implement-code", "--stream"],
    );

    assert!(output.status.success());
    let streamed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!streamed.contains("cross-stream-secret"));
    let logs = fs::read_dir(project.path().join("src/.kvist/logs"))
        .expect("read logs")
        .next()
        .expect("log")
        .expect("log entry")
        .path();
    let log = fs::read_to_string(logs).expect("read log");
    assert!(!log.contains("cross-stream-secret"), "log: {log}");
    assert!(log.contains("[REDACTED]"), "log: {log}");
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_redacts_all_verification_evidence_blockers_and_cli_output() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo agent"
[agent.profiles.developer.redaction]
values = ["agent-redaction-secret"]

[sandbox]
schema_version = 1
runner = "REPLACED_BY_TEST"
network = "deny"
environment_allowlist = ["KVIST_TEST_SECRET"]
mount = "component"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["KVIST_TEST_SECRET"]
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "verification-secret"
"#,
    )
    .expect("write config");
    configure_fake_sandbox(&project);
    let config_path = project.path().join("kvist.toml");
    let config = fs::read_to_string(&config_path)
        .expect("read config")
        .replace(
            "REPLACED_BY_TEST",
            &fake_sandbox_runner_path(&project).display().to_string(),
        );
    fs::write(config_path, config).expect("configure runner");
    track_project(&project);
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(["task", "run", ".", "implement-code"])
        .env("KVIST_TEST_SECRET", "sandbox-environment-secret")
        .current_dir(project.path())
        .output()
        .expect("run task");
    assert!(output.status.success());
    let cli_output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let queue = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    let attempts = fs::read_to_string(
        project
            .path()
            .join("src/.kvist-attempts/implement-code.jsonl"),
    )
    .expect("read attempts");
    for evidence in [&cli_output, &queue, &attempts] {
        assert!(!evidence.contains("agent-redaction-secret"), "{evidence}");
        assert!(
            !evidence.contains("sandbox-environment-secret"),
            "{evidence}"
        );
    }
    assert!(attempts.contains("\"phase\":\"verification\""));
    assert!(queue.contains("status: blocked"));
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_lifecycle_lock_survives_agent_component_lock_deletion() {
    use std::{thread, time::Duration};

    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    fs::write(
        project.path().join("kvist.toml"),
        r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "delete-lock"
timeout_seconds = 5
[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = []
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo verify"
"#,
    )
    .expect("write config");
    track_project(&project);
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );

    let mut first = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(["task", "run", ".", "implement-code"])
        .current_dir(project.path())
        .spawn()
        .expect("start first run");
    let request = project.path().join("sandbox-request.json");
    for _ in 0..100 {
        if fs::read_to_string(&request).is_ok_and(|contents| contents.contains("delete-lock")) {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        fs::read_to_string(&request).is_ok_and(|contents| contents.contains("delete-lock")),
        "first run did not reach sandboxed execution"
    );
    assert!(!project.path().join("src/.kvist-task.lock").exists());

    let second = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("lock"));
    let transition = run_kvist(
        &project,
        &[
            "task",
            "transition",
            ".",
            "implement-code",
            "blocked",
            "--reason",
            "manual",
        ],
    );
    assert!(!transition.status.success());
    assert!(String::from_utf8_lossy(&transition.stderr).contains("lock"));
    let intermediate =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(intermediate.contains("status: in-progress"));

    assert!(first.wait().expect("wait first").success());
    let queue = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue.contains("status: completed"));
    let attempts = fs::read_to_string(
        project
            .path()
            .join("src/.kvist-attempts/implement-code.jsonl"),
    )
    .expect("read attempts");
    assert_eq!(attempts.matches("\"phase\":\"agent-execution\"").count(), 1);
}

#[test]
#[cfg(target_os = "linux")]
fn task_log_reads_and_outputs_the_most_recent_log_file() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // Configure a developer agent that echoes some specific log content
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo 'my expected log output'"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo 'mocking verify'"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);

    // Approve test policy
    let approve_output = run_kvist(&project, &["task", "approve-policy"]);
    assert!(approve_output.status.success());

    // 1. Run the task to generate the execution log
    let run_output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(
        run_output.status.success(),
        "Run failed: {}",
        String::from_utf8_lossy(&run_output.stderr)
    );

    // 2. Read the log via task log CLI and assert correct output
    let log_output = run_kvist(&project, &["task", "log", ".", "implement-code"]);
    assert!(
        log_output.status.success(),
        "Log command failed: {}",
        String::from_utf8_lossy(&log_output.stderr)
    );
    let stdout_str = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        stdout_str.contains("fake sandbox runner"),
        "Expected log contents not found in output: {}",
        stdout_str
    );
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_fails_with_unapproved_test_policy() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // Configure test policy but do NOT approve it
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo 'mocking execute' {context_files}"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "echo 'mocking verify'"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);

    // Run task - it must refuse before sandbox probing or task mutation.
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("approval record"));

    let queue_contents =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue_contents.contains("status: pending"));
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_fails_with_missing_test_command() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // Configure test policy but NO commands matching .
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo 'mocking execute' {context_files}"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 1000
commands = []
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);

    // Approve the policy
    let approve_output = run_kvist(&project, &["task", "approve-policy"]);
    assert!(approve_output.status.success());

    // Run task - should block due to missing command for .
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(output.status.success());
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    assert!(stdout_str.contains(
        "verification blocked and transitioned to blocked: missing test-command policy for component `.`"
    ));

    let queue_contents =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue_contents.contains("status: blocked"));
    assert!(queue_contents.contains("missing test-command policy for component `.`"));
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_fails_when_test_command_fails() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // Configure test policy with a failing test command (false/exit 1)
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo 'mocking execute' {context_files}"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "false"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);

    // Approve policy
    let approve_output = run_kvist(&project, &["task", "approve-policy"]);
    assert!(approve_output.status.success());

    // Run task - should run verification, fail, and block
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(output.status.success());
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    assert!(stdout_str.contains("failed test-command verification and transitioned to blocked"));

    let queue_contents =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue_contents.contains("status: blocked"));
    assert!(queue_contents.contains("test-command verification failed"));
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_handles_test_command_timeout() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");

    // Configure test policy with a 1 second timeout and a sleeping test command
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo 'mocking execute' {context_files}"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 1
max_output_bytes = 1000
[[test_policy.commands]]
component = "."
command = "sleep 5"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);

    // Approve policy
    let approve_output = run_kvist(&project, &["task", "approve-policy"]);
    assert!(approve_output.status.success());

    // Run task - should timeout, kill command, and block
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(output.status.success());
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    assert!(stdout_str.contains("failed test-command verification and transitioned to blocked"));

    let queue_contents =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue_contents.contains("status: blocked"));
    assert!(queue_contents.contains("(timed out)"));
}

#[test]
#[cfg(target_os = "linux")]
fn task_run_caps_test_command_output_and_records_persistence() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("write queue");
    fs::write(
        project.path().join("src/emit-and-fail.sh"),
        "printf 1234567890abcdef\nfalse\n",
    )
    .expect("write test command script");

    // Configure test policy with max_output_bytes = 10 and a command that outputs a long string
    // we use a failing command so we can inspect stdout in the blocked reason
    let config_toml = r#"schema_version = 1
component_root = "src"
[agent.profiles.developer]
command_template = "echo 'mocking execute' {context_files}"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 10
[[test_policy.commands]]
component = "."
command = "sh ./emit-and-fail.sh"
"#;
    fs::write(project.path().join("kvist.toml"), config_toml).expect("write config");
    track_project(&project);

    // Approve policy
    let approve_output = run_kvist(&project, &["task", "approve-policy"]);
    assert!(approve_output.status.success());

    // Run task
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    assert!(output.status.success());

    // Verify task status is indeed Blocked in TODOS.yaml and the recorded reason is capped
    let queue_contents =
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("read queue");
    assert!(queue_contents.contains("status: blocked"));

    // The captured stdout in the blocker reason should be exactly 10 bytes: "1234567890" (ignoring any newline cap depending on shell)
    assert!(queue_contents.contains("1234567890"));

    // Check result-persistence: inspect task attempt JSONL file
    let attempt_file = project
        .path()
        .join("src/.kvist-attempts/implement-code.jsonl");
    assert!(attempt_file.exists());
    let attempt_contents = fs::read_to_string(&attempt_file).expect("read attempts");
    assert!(attempt_contents.contains(r#""phase":"verification""#));
    if !(attempt_contents.contains(r#""stdout":"1234567890""#)
        || attempt_contents.contains(r#""stdout":"1234567890\n""#))
    {
        panic!("Assertion failed! attempt_contents: \n{}", attempt_contents);
    }
}
