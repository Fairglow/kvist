use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::{Builder, TempDir};

pub const TASK_ID: &str = "implement-code";
pub const ATTEMPT_ID: &str = "attempt-0001";
pub const ACCEPTANCE_ID: &str = "acceptance-0001";

pub struct TargetProject {
    project: TempDir,
    external_tools: TempDir,
}

impl TargetProject {
    pub fn path(&self) -> &Path {
        self.project.path()
    }

    pub fn external_tools_path(&self) -> &Path {
        self.external_tools.path()
    }
}

pub fn repository_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if manifest.join("VISION.md").is_file() {
        manifest
    } else if manifest
        .parent()
        .is_some_and(|parent| parent.join("VISION.md").is_file())
    {
        manifest.parent().expect("manifest parent").to_path_buf()
    } else {
        panic!(
            "cannot locate the Kvist repository root from {}",
            manifest.display()
        );
    }
}

fn engine_authority_root() -> PathBuf {
    repository_root().join("engine")
}

pub fn temporary_directory(prefix: &str) -> TempDir {
    Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("create operating-system temporary directory")
}

pub fn run_kvist(project: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .current_dir(project)
        .output()
        .expect("run kvist")
}

pub fn run_kvist_with_path(project: &Path, arguments: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .current_dir(project)
        .env("PATH", path)
        .output()
        .expect("run kvist with controlled PATH")
}

pub fn run(program: &str, arguments: &[&str], directory: &Path) -> Output {
    Command::new(program)
        .args(arguments)
        .current_dir(directory)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .unwrap_or_else(|error| panic!("run {program}: {error}"))
}

pub fn git(project: &Path, arguments: &[&str]) -> Output {
    run("git", arguments, project)
}

pub fn assert_success(output: &Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

pub fn sha256_file(path: &Path) -> String {
    sha256_bytes(&fs::read(path).expect("read digest input"))
}

fn copy_authority(project: &Path, name: &str) {
    fs::copy(repository_root().join(name), project.join(name))
        .unwrap_or_else(|error| panic!("copy {name}: {error}"));
}

fn copy_engine_artifact(engine: &Path, name: &str) {
    fs::copy(engine_authority_root().join(name), engine.join(name))
        .unwrap_or_else(|error| panic!("copy engine/{name}: {error}"));
}

fn queue(engine: &Path, status: &str, recovery_state: &str) -> String {
    format!(
        r#"schema_version: 1
component:
  requirements_revision: "{}"
  contract_revision: "{}"
  design_revision: "{}"
  parent_contract: null
  revalidation:
    state: current
    checked_at: "2026-09-01T20:30:00Z"
    stale_since: null
    causes: []
tasks:
  - id: "write-tests"
    title: "Write fixture tests"
    description: "Define the fixture coverage before implementation."
    context: "The recovery fixture needs a completed lifecycle predecessor."
    purpose: "Keep every fixture queue valid under lifecycle validation."
    expected_outcome: "The prerequisite test task is complete."
    kind: test
    status: completed
    depends_on: []
    requirements:
      - "REQUIREMENTS.md#REQ-SUPERVISED-EXECUTION"
    timestamps:
      created_at: "2026-09-01T20:30:00Z"
      updated_at: "2026-09-01T20:30:00Z"
      completed_at: "2026-09-01T20:30:00Z"
    blocked_reason: null
    recovery_state: null
  - id: "{TASK_ID}"
    title: "Implement fixture"
    description: "Implement only the accepted fixture scope."
    context: "The dogfooding boundary test owns this isolated repository."
    purpose: "Exercise supervised execution and exact acceptance."
    expected_outcome: "The attempt remains pending until explicit human finalization."
    kind: implementation
    status: {status}
    depends_on:
      - "write-tests"
    requirements:
      - "REQUIREMENTS.md#REQ-SUPERVISED-EXECUTION"
    timestamps:
      created_at: "2026-09-01T20:30:00Z"
      updated_at: "2026-09-01T20:30:00Z"
      completed_at: null
    blocked_reason: null
    recovery_state: {recovery_state}
"#,
        sha256_file(&engine.join("REQUIREMENTS.md")),
        sha256_file(&engine.join("CONTRACT.md")),
        sha256_file(&engine.join("DESIGN.md")),
    )
}

pub fn task_block<'a>(queue: &'a str, task_id: &str) -> &'a str {
    let marker = format!("  - id: \"{task_id}\"");
    let (_, task_and_following) = queue
        .split_once(&marker)
        .unwrap_or_else(|| panic!("queue must contain task `{task_id}`"));
    task_and_following
        .split("\n  - id: ")
        .next()
        .expect("task block must end at the next declaration or end of queue")
}

pub fn add_independent_ready_implementation_task(project: &Path) {
    let queue_path = project.join("engine/TODOS.yaml");
    let mut queue = fs::read_to_string(&queue_path).expect("read fixture queue");
    queue.push_str(
        r#"  - id: "independent-implementation"
    title: "Implement independent fixture"
    description: "Exercise a legal unrelated queue mutation."
    context: "The completed test predecessor makes this task independently ready."
    purpose: "Prove a fenced sibling prevents component-wide queue rewrites."
    expected_outcome: "The task remains independently runnable once recovery completes."
    kind: implementation
    status: pending
    depends_on:
      - "write-tests"
    requirements:
      - "REQUIREMENTS.md#REQ-SUPERVISED-EXECUTION"
    timestamps:
      created_at: "2026-09-01T20:30:00Z"
      updated_at: "2026-09-01T20:30:00Z"
      completed_at: null
    blocked_reason: null
    recovery_state: null
"#,
    );
    fs::write(queue_path, queue).expect("add independent fixture task");
}

fn create_child(engine: &Path, name: &str, package: &str) {
    let child = engine.join(name);
    fs::create_dir_all(child.join("src")).expect("create child source directory");
    for artifact in ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md", "IMPL.md"] {
        copy_engine_artifact(&child, artifact);
    }
    let child_queue = format!(
        r#"schema_version: 1
component:
  requirements_revision: "{}"
  contract_revision: "{}"
  design_revision: "{}"
  parent_contract:
    path: "../CONTRACT.md"
    revision: "{}"
  revalidation:
    state: current
    checked_at: "2026-09-01T20:30:00Z"
    stale_since: null
    causes: []
tasks: []
"#,
        sha256_file(&child.join("REQUIREMENTS.md")),
        sha256_file(&child.join("CONTRACT.md")),
        sha256_file(&child.join("DESIGN.md")),
        sha256_file(&engine.join("CONTRACT.md")),
    );
    fs::write(child.join("TODOS.yaml"), child_queue).expect("write child queue");
    fs::write(
        child.join("Cargo.toml"),
        format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
    )
    .expect("write child manifest");
    fs::write(child.join("src/lib.rs"), "pub fn fixture() {}\n").expect("write child source");
    fs::create_dir(child.join("tests")).expect("create child tests");
}

fn write_controlled_runner(project: &Path, external_tools: &Path) -> PathBuf {
    let runner = external_tools.join("controlled-sandbox-runner");
    let request_log = external_tools.join("runner-request.json");
    let backend = Path::new("/usr/bin/true");
    let script = format!(
        r#"#!/usr/bin/bash
set -eu
if [ "${{1-}}" = "--kvist-sandbox-probe-v1" ]; then
  runner_digest=$(sha256sum "$0" | cut -d' ' -f1)
  backend_digest=$(sha256sum "{backend}" | cut -d' ' -f1)
  printf '{{"protocol":"kvist-sandbox-probe-v1","protocol_version":1,"runner":{{"path":"%s","digest":"sha256:%s"}},"backend":{{"kind":"bubblewrap","path":"{backend}","digest":"sha256:%s"}},"capabilities":{{"namespaces":{{"mount":true,"network":true,"pid":true,"ipc":true,"uts":true,"user":true}},"new_session":true,"parent_death_signal":true}}}}\n' "$0" "$runner_digest" "$backend_digest"
  exit 0
fi
cat > "{request_log}"
printf 'started\n' > "{project}/engine/tests/supervised-run-started"
printf '#[test]\nfn generated() {{}}\n' > "{project}/engine/tests/generated.rs"
"#,
        backend = backend.display(),
        request_log = request_log.display(),
        project = project.display(),
    );
    fs::write(&runner, script).expect("write controlled sandbox runner");
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755))
        .expect("make controlled sandbox runner executable");
    runner
}

pub fn create_target_project(status: &str) -> TargetProject {
    let project = temporary_directory("project-");
    let external_tools = temporary_directory("dogfood-tools-");
    for artifact in ["VISION.md", "ARCHITECTURE.md", "ROOT_CONTRACT.md"] {
        copy_authority(project.path(), artifact);
    }
    fs::create_dir(project.path().join("docs")).expect("create docs");
    fs::create_dir_all(project.path().join("engine/src")).expect("create engine source");
    fs::create_dir(project.path().join("engine/tests")).expect("create engine tests");
    let engine = project.path().join("engine");
    for artifact in ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md", "IMPL.md"] {
        copy_engine_artifact(&engine, artifact);
    }
    fs::write(engine.join("TODOS.yaml"), queue(&engine, status, "null"))
        .expect("write engine queue");
    fs::write(
        engine.join("Cargo.toml"),
        r#"[package]
name = "dogfood-fixture"
version = "0.1.0"
edition = "2024"

[workspace]
members = [".", "agent_runtime", "sandbox_runner"]
resolver = "3"
"#,
    )
    .expect("write engine manifest");
    fs::write(engine.join("Cargo.lock"), "version = 4\n").expect("write lockfile");
    fs::write(engine.join("src/lib.rs"), "pub fn existing() {}\n").expect("write engine source");
    create_child(&engine, "agent_runtime", "fixture-agent-runtime");
    create_child(&engine, "sandbox_runner", "fixture-sandbox-runner");
    let runner = write_controlled_runner(project.path(), external_tools.path());
    fs::write(
        project.path().join("kvist.toml"),
        format!(
            r##"schema_version = 1
component_root = "engine"

[review]
required = false

[vcs]
kind = "git"

[agent.profiles.developer]
command_template = "/usr/bin/bash -c 'printf started > tests/supervised-agent-started; printf \"#[test]\\nfn generated() {{}}\\n\" > tests/generated.rs'"
timeout_seconds = 5
max_output_bytes = 65536

[sandbox]
schema_version = 1
runner = "{}"
network = "deny"
environment_allowlist = ["PATH"]
mount = "component"

[test_policy]
schema_version = 1
working_directory = "component"
environment_allowlist = ["PATH"]
timeout_seconds = 5
max_output_bytes = 65536

[[test_policy.commands]]
component = "."
command = "/usr/bin/test -f tests/generated.rs"
"##,
            runner.display()
        ),
    )
    .expect("write project configuration");

    assert_success(&git(project.path(), &["init", "--quiet"]), "git init");
    assert_success(
        &git(
            project.path(),
            &["config", "user.name", "Kvist Boundary Tests"],
        ),
        "configure git name",
    );
    assert_success(
        &git(
            project.path(),
            &["config", "user.email", "tests@example.invalid"],
        ),
        "configure git email",
    );
    assert_success(&git(project.path(), &["add", "--all"]), "stage fixture");
    assert_success(
        &git(
            project.path(),
            &[
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "fixture base",
            ],
        ),
        "commit fixture",
    );
    TargetProject {
        project,
        external_tools,
    }
}

pub fn write_ambiguous_attempt(project: &Path) {
    let engine = project.join("engine");
    fs::write(
        engine.join("TODOS.yaml"),
        queue(
            &engine,
            "in-progress",
            &format!(
                "{{ state: fenced, attempt_id: \"{ATTEMPT_ID}\", reason: \"ambiguous-effects\" }}"
            ),
        ),
    )
    .expect("write fenced queue");
    let attempts = engine.join(".kvist-attempts");
    fs::create_dir_all(&attempts).expect("create attempt journal directory");
    let pre_queue = git(project, &["show", "HEAD:engine/TODOS.yaml"]);
    assert_success(&pre_queue, "read pre-attempt queue");
    let event = json!({
        "schema_version": 1,
        "attempt_id": ATTEMPT_ID,
        "task_id": TASK_ID,
        "phase": "prepared",
        "pre_queue_digest": sha256_bytes(&pre_queue.stdout),
        "intended_post_queue_digest": sha256_file(&engine.join("TODOS.yaml")),
        "policy_identity": sha256_bytes(b"policy"),
        "runner_identity": sha256_bytes(b"runner"),
        "approved_write_scope": [{
            "path": "engine/tests",
            "pre_digest": sha256_bytes(b"empty")
        }]
    });
    fs::write(
        attempts.join(format!("{TASK_ID}.jsonl")),
        format!("{event}\n"),
    )
    .expect("write ambiguous attempt");
    fs::write(engine.join("tests/uncertain.rs"), "// uncertain effect\n")
        .expect("write ambiguous source effect");
}

pub fn write_finalizable_attempt(project: &Path, verification_success: bool) {
    let engine = project.join("engine");
    fs::write(
        engine.join("TODOS.yaml"),
        queue(&engine, "in-progress", "null"),
    )
    .expect("write in-progress queue");
    fs::write(
        engine.join("tests/generated.rs"),
        "#[test]\nfn generated() {}\n",
    )
    .expect("write scoped task result");
    let attempts = engine.join(".kvist-attempts");
    fs::create_dir_all(&attempts).expect("create attempt journal directory");
    let changes = json!([{
        "path": "engine/tests/generated.rs",
        "operation": "create",
        "pre_digest": Value::Null,
        "post_digest": sha256_file(&engine.join("tests/generated.rs"))
    }]);
    let events = [
        json!({
            "schema_version": 1,
            "attempt_id": ATTEMPT_ID,
            "task_id": TASK_ID,
            "phase": "prepared",
            "pre_queue_digest": sha256_bytes(
                &git(project, &["show", "HEAD:engine/TODOS.yaml"]).stdout
            ),
            "intended_post_queue_digest": sha256_file(&engine.join("TODOS.yaml")),
            "policy_identity": sha256_bytes(b"engine-approved-policy"),
            "runner_identity": sha256_bytes(b"engine-approved-runner"),
            "approved_write_scope": [{
                "path": "engine/tests",
                "pre_digest": sha256_bytes(b"empty")
            }]
        }),
        json!({
            "schema_version": 1,
            "attempt_id": ATTEMPT_ID,
            "task_id": TASK_ID,
            "phase": "execution-finished",
            "result": "success",
            "changes": changes
        }),
        json!({
            "schema_version": 1,
            "attempt_id": ATTEMPT_ID,
            "task_id": TASK_ID,
            "phase": "verification-finished",
            "result": if verification_success { "success" } else { "failure" }
        }),
        json!({
            "schema_version": 1,
            "attempt_id": ATTEMPT_ID,
            "task_id": TASK_ID,
            "phase": "pending-human-disposition"
        }),
    ];
    fs::write(
        attempts.join(format!("{TASK_ID}.jsonl")),
        events
            .iter()
            .map(|event| format!("{event}\n"))
            .collect::<String>(),
    )
    .expect("write finalizable attempt");
}

pub fn runner_path() -> PathBuf {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("KVIST_SANDBOX_RUNNER_TEST_BIN") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_sandbox-runner") {
        candidates.push(PathBuf::from(path));
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_kvist-sandbox-runner") {
        candidates.push(PathBuf::from(path));
    }
    let root = repository_root();
    if let Ok(executable) = std::env::current_exe()
        && let Some(debug_directory) = executable.parent().and_then(Path::parent)
    {
        candidates.push(debug_directory.join("sandbox-runner"));
        candidates.push(debug_directory.join("kvist-sandbox-runner"));
    }
    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(&target).join("debug/sandbox-runner"));
        candidates.push(PathBuf::from(target).join("debug/kvist-sandbox-runner"));
    }
    for relative in [
        "engine/target/debug/sandbox-runner",
        "engine/target/debug/kvist-sandbox-runner",
        "target/debug/sandbox-runner",
        "target/debug/kvist-sandbox-runner",
    ] {
        candidates.push(root.join(relative));
    }
    let canonical_worktree = repository_root()
        .canonicalize()
        .expect("canonical test worktree");
    candidates
        .into_iter()
        .find(|path| {
            fs::symlink_metadata(path)
                .ok()
                .and_then(|metadata| {
                    path.canonicalize().ok().map(|canonical| {
                        metadata.file_type().is_file()
                            && !metadata.file_type().is_symlink()
                            && !canonical.starts_with(&canonical_worktree)
                    })
                })
                .unwrap_or(false)
        })
        .unwrap_or_else(|| {
            panic!(
                "the real Bubblewrap sandbox runner is not installed outside the selected \
                 worktree; build engine/sandbox_runner and set KVIST_SANDBOX_RUNNER_TEST_BIN \
                 to its installed regular-file executable"
            )
        })
}

pub fn probe_runner() -> Value {
    let runner = runner_path();
    let output = Command::new(&runner)
        .arg("--kvist-sandbox-probe-v1")
        .env_clear()
        .output()
        .expect("run real sandbox probe");
    assert_success(&output, "real Bubblewrap sandbox probe");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "sandbox probe must emit one canonical JSON object: {error}; stdout was {:?}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

pub fn run_runner_request(request: &Value) -> Output {
    let runner = runner_path();
    let mut child = Command::new(runner)
        .arg("--kvist-sandbox-request-v1")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start real sandbox runner");
    child
        .stdin
        .take()
        .expect("runner stdin")
        .write_all(&serde_json::to_vec(request).expect("encode request"))
        .expect("write sandbox request");
    child.wait_with_output().expect("wait for sandbox runner")
}

pub fn toolchain_grant() -> Value {
    json!({
        "source": "/usr",
        "destination": "/usr",
        "access": "read-only",
        "purpose": "toolchain",
        "identity": sha256_bytes(b"approved-host-usr-toolchain")
    })
}

pub fn base_request(fixture: &Path, phase: &str, argv: &[&str]) -> Value {
    let probe = probe_runner();
    let runner_digest = probe
        .pointer("/runner/digest")
        .and_then(Value::as_str)
        .expect("probe runner.digest");
    let backend_path = probe
        .pointer("/backend/path")
        .and_then(Value::as_str)
        .expect("probe backend.path");
    let backend_digest = probe
        .pointer("/backend/digest")
        .and_then(Value::as_str)
        .expect("probe backend.digest");
    json!({
        "protocol": "kvist-sandbox-request-v1",
        "protocol_version": 1,
        "phase": phase,
        "argv": argv,
        "working_directory": "/workspace/component",
        "environment": {
            "HOME": "/workspace/home",
            "PATH": "/usr/bin"
        },
        "network": {
            "mode": "deny",
            "allowed_sources": []
        },
        "resources": {
            "wall_time_ms": 5000,
            "max_output_bytes": 65536,
            "max_processes": 16,
            "max_files": 256,
            "max_file_bytes": 1048576,
            "max_scratch_bytes": 1048576
        },
        "identities": {
            "runner": runner_digest,
            "backend": {
                "kind": "bubblewrap",
                "path": backend_path,
                "digest": backend_digest
            },
            "policy": sha256_bytes(b"policy"),
            "toolchain": sha256_bytes(b"toolchain"),
            "command": sha256_bytes(serde_json::to_string(argv).expect("argv").as_bytes()),
            "mount_plan": sha256_bytes(fixture.to_string_lossy().as_bytes())
        },
        "grants": [
            {
                "source": fixture.join("REQUIREMENTS.md").canonicalize().expect("requirements"),
                "destination": "/workspace/component/REQUIREMENTS.md",
                "access": "read-only",
                "purpose": "context",
                "identity": sha256_file(&fixture.join("REQUIREMENTS.md"))
            },
            {
                "source": fixture.join("tests").canonicalize().expect("tests"),
                "destination": "/workspace/component/tests",
                "access": "read-write",
                "purpose": "authoring",
                "identity": sha256_bytes(b"approved-tests-directory")
            },
            toolchain_grant(),
            {
                "source": fixture.join("scratch").canonicalize().expect("scratch"),
                "destination": "/workspace/scratch",
                "access": "read-write",
                "purpose": "scratch",
                "identity": sha256_bytes(b"attempt-scratch")
            }
        ],
        "toolchain": {
            "kind": "system",
            "identity": sha256_bytes(b"toolchain"),
            "root": "/usr"
        },
        "cache": null,
        "scratch": {
            "destination": "/workspace/scratch",
            "identity": sha256_bytes(b"attempt-scratch")
        }
    })
}

pub fn sandbox_fixture() -> TempDir {
    let fixture = temporary_directory("sandbox-");
    fs::create_dir(fixture.path().join("tests")).expect("create writable tests");
    fs::create_dir(fixture.path().join("scratch")).expect("create scratch");
    fs::create_dir_all(fixture.path().join("child/src")).expect("create child implementation");
    fs::create_dir(fixture.path().join(".git")).expect("create protected Git metadata");
    fs::create_dir(fixture.path().join(".kvist")).expect("create protected evidence");
    fs::write(fixture.path().join("REQUIREMENTS.md"), "protected intent\n").expect("write intent");
    fs::write(fixture.path().join("TODOS.yaml"), "protected queue\n").expect("write queue");
    fs::write(fixture.path().join("IMPL.md"), "protected record\n").expect("write record");
    fs::write(fixture.path().join(".git/index"), "protected git index\n").expect("write Git state");
    fs::write(
        fixture.path().join(".kvist/evidence.json"),
        "protected evidence\n",
    )
    .expect("write evidence");
    fs::write(fixture.path().join("child/src/lib.rs"), "protected child\n")
        .expect("write child implementation");
    fixture
}
