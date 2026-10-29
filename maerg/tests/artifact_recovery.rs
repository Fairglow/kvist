use std::{fs, path::Path, process::Command};

use kvist::{init::initialize, task_queue};
use serde_json::Value;
use tempfile::TempDir;

fn run(project: &Path, args: &[&str]) -> (bool, Value) {
    let state = TempDir::new().expect("private user state");
    run_in_state(project, args, state.path())
}

fn run_in_state(project: &Path, args: &[&str], state: &Path) -> (bool, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .env("XDG_STATE_HOME", state)
        .args(["--json", "repair"])
        .arg(project)
        .args(args)
        .output()
        .expect("run recovery");
    let text = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let report = serde_json::from_slice(text)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}: {}", String::from_utf8_lossy(text)));
    (output.status.success(), report)
}

#[test]
fn live_stale_and_malformed_operation_locks_are_never_silently_removed() {
    use sha2::{Digest, Sha256};
    let project = fixture();
    let state = TempDir::new().expect("user state");
    let directory = state.path().join("kvist/task-locks-v1");
    fs::create_dir_all(&directory).expect("lock directory");
    let identity = hex::encode(Sha256::digest(
        format!(
            "{}\n{}",
            project.path().display(),
            project.path().join("src").display()
        )
        .as_bytes(),
    ));
    let lock = directory.join(format!("{identity}.lock"));
    fs::write(project.path().join("src/TODOS.yaml"), "[broken").expect("broken");
    for contents in [
        format!("schema_version: 1\nstarted_at: 2026-10-29T10:00:00Z\ntask_id: \"old-task\"\npid: {}\nnonce: 0123456789abcdef0123456789abcdef\n", std::process::id()),
        "schema_version: 1\nstarted_at: 2026-10-29T10:00:00Z\ntask_id: \"old-task\"\npid: 4294967295\nnonce: 0123456789abcdef0123456789abcdef\n".to_owned(),
        "malformed retained lock".to_owned(),
    ] {
        fs::write(&lock, &contents).expect("retained lock");
        let (_, preview) = run_in_state(project.path(), &["--clean-slate"], state.path());
        let (ok, report) = run_in_state(project.path(), &["--clean-slate", "--approve", preview["approval"].as_str().expect("token")], state.path());
        assert!(!ok, "{report}");
        assert_eq!(fs::read_to_string(&lock).expect("lock"), contents);
        assert_eq!(fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue"), "[broken");
        assert!(!project.path().join(".kvist-recovery").exists());
    }
}

#[test]
fn markdown_salvage_preserves_prose_and_repairs_required_structure_before_reset() {
    let project = fixture();
    let original = "# Existing requirements\nKeep this introduction.\n\n## Functional requirements\nKeep functional intent.\n\n## Purpose and scope\nKeep scope.\n\n## Purpose and scope\nKeep duplicate scope too.\n\n## Additional material\nKeep this optional section.\n";
    fs::write(project.path().join("src/REQUIREMENTS.md"), original).expect("document");
    let (_, preview) = run(project.path(), &["--salvage"]);
    assert!(preview["approval"].is_string(), "{preview}");
    assert_eq!(
        fs::read_to_string(project.path().join("src/REQUIREMENTS.md")).expect("unchanged"),
        original
    );
    let (ok, report) = approve(project.path(), &["--salvage"], &preview);
    assert!(ok, "{report}");
    let contents =
        fs::read_to_string(project.path().join("src/REQUIREMENTS.md")).expect("document");
    for text in [
        "Keep this introduction.",
        "Keep functional intent.",
        "Keep scope.",
        "Keep duplicate scope too.",
        "Keep this optional section.",
    ] {
        assert!(contents.contains(text), "{text}");
    }
    assert!(contents.contains("Recovery draft"));
    assert!(
        kvist::component_documents::validate(
            kvist::component_documents::DocumentKind::Requirements,
            &contents
        )
        .is_valid()
    );
}

#[test]
fn markdown_salvage_does_not_relabel_known_unsupported_versions() {
    for (prefix, version) in [("", 99), ("\u{feff}", 99), ("\n", 99), ("", 0)] {
        let project = fixture();
        let original = format!(
            "{prefix}<!-- kvist-vision-version: {version} -->\n# Project Vision\nPreserve unsupported format.\n"
        );
        fs::write(project.path().join("VISION.md"), &original).expect("future");
        let (ok, report) = run(project.path(), &["--salvage"]);
        assert!(!ok);
        assert!(report["approval"].is_null(), "{report}");
        assert_eq!(
            fs::read_to_string(project.path().join("VISION.md")).expect("untouched"),
            original
        );
    }
}

#[test]
fn ignore_policy_changes_invalidate_an_existing_approval() {
    let project = fixture();
    fs::write(project.path().join("VISION.md"), "broken").expect("broken");
    let (_, preview) = run(project.path(), &["--reset", "VISION.md"]);
    fs::write(project.path().join(".gitignore"), "unrelated-directory/\n").expect("policy");
    let (ok, report) = approve(project.path(), &["--reset", "VISION.md"], &preview);
    assert!(!ok, "{report}");
    assert_eq!(
        fs::read_to_string(project.path().join("VISION.md")).expect("untouched"),
        "broken"
    );
    assert!(!project.path().join(".kvist-recovery").exists());
}

#[test]
fn root_and_implementation_record_formatting_can_be_salvaged_without_erasing_content() {
    for path in [
        "VISION.md",
        "ARCHITECTURE.md",
        "ROOT_CONTRACT.md",
        "src/IMPL.md",
    ] {
        let project = fixture();
        fs::write(
            project.path().join(path),
            "Original material must remain.\n",
        )
        .expect("document");
        let (_, preview) = run(project.path(), &["--salvage"]);
        let (ok, report) = approve(project.path(), &["--salvage"], &preview);
        assert!(ok, "{path}: {report}");
        assert!(
            fs::read_to_string(project.path().join(path))
                .expect("document")
                .contains("Original material must remain.")
        );
    }
}

#[test]
fn ignored_scratch_worktrees_are_not_recovered_or_included_in_clean_slate() {
    let project = fixture();
    fs::write(project.path().join(".gitignore"), ".review-scratch/\n").expect("policy");
    fs::create_dir_all(project.path().join(".review-scratch/other-project")).expect("scratch");
    let foreign = project
        .path()
        .join(".review-scratch/other-project/TODOS.yaml");
    fs::write(&foreign, "[foreign broken data").expect("foreign");
    let (ok, report) = run(project.path(), &[]);
    assert!(ok, "ignored worktrees must not affect repair: {report}");
    let (_, preview) = run(project.path(), &["--clean-slate"]);
    assert!(
        !preview.to_string().contains(".review-scratch"),
        "{preview}"
    );
    let (ok, report) = approve(project.path(), &["--clean-slate"], &preview);
    assert!(ok, "{report}");
    assert_eq!(
        fs::read_to_string(foreign).expect("untouched"),
        "[foreign broken data"
    );
}

fn fixture() -> TempDir {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    project
}

fn queue(project: &Path, tasks: &str) {
    let path = project.join("src/TODOS.yaml");
    let original = fs::read_to_string(&path).expect("queue");
    let prefix = original.split("tasks:").next().expect("metadata");
    fs::write(path, format!("{prefix}tasks:\n{tasks}")).expect("write fixture");
}

fn task(id: &str, dependencies: &str) -> String {
    format!(
        r#"  - id: {id}
    title: Test {id}
    description: Preserve useful work.
    context: Recovery fixture.
    purpose: Check recovery.
    expected_outcome: A strictly valid queue.
    kind: test
    status: completed
    depends_on: [{dependencies}]
    requirements: [REQUIREMENTS.md#Purpose]
    timestamps:
      created_at: 2026-10-29T10:00:00Z
      updated_at: 2026-10-29T10:00:00Z
      completed_at: 2026-10-29T10:00:00Z
    blocked_reason: null
    recovery_state: null
"#
    )
}

fn approve(project: &Path, options: &[&str], preview: &Value) -> (bool, Value) {
    let token = preview["approval"].as_str().expect("approval token");
    let mut args = options.to_vec();
    args.extend(["--approve", token]);
    run(project, &args)
}

#[test]
fn completion_timestamp_reconciliation_requires_exact_approval_and_backup() {
    let project = fixture();
    let content = task("keep", "").replace(
        "updated_at: 2026-10-29T10:00:00Z",
        "updated_at: 2026-10-29T10:01:00Z",
    );
    queue(project.path(), &content);
    let path = project.path().join("src/TODOS.yaml");
    let before = fs::read(&path).expect("original");
    let (ok, preview) = run(project.path(), &["--salvage"]);
    assert!(!ok);
    assert_eq!(fs::read(&path).expect("unchanged"), before);
    assert!(!project.path().join(".kvist-recovery").exists());
    assert!(preview.to_string().contains("completed_at"));
    let (ok, applied) = approve(project.path(), &["--salvage"], &preview);
    assert!(ok, "{applied}");
    let backup = applied["backup"].as_str().expect("backup");
    assert_eq!(
        fs::read(Path::new(backup).join("originals/src/TODOS.yaml")).expect("backup bytes"),
        before
    );
    let repaired = task_queue::parse(&fs::read_to_string(&path).expect("repaired")).expect("valid");
    assert_eq!(
        repaired.tasks[0].timestamps.completed_at.as_ref(),
        Some(&repaired.tasks[0].timestamps.updated_at)
    );
    assert_eq!(
        repaired.component.revalidation.state,
        task_queue::RevalidationState::Stale
    );
    let (_, second) = run(project.path(), &[]);
    assert!(second["plan"].as_array().expect("plan").is_empty());
}

#[test]
fn dependency_order_is_repaired_without_removing_tasks() {
    let project = fixture();
    queue(
        project.path(),
        &(task("later", "earlier") + &task("earlier", "")),
    );
    let (ok, report) = run(project.path(), &[]);
    assert!(ok, "{report}");
    let content = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue");
    let repaired = task_queue::parse(&content).expect("valid");
    assert_eq!(repaired.tasks[0].id, "earlier");
    assert_eq!(repaired.tasks[1].id, "later");
    assert!(report["backup"].is_string());
}

#[test]
fn salvage_preserves_valid_work_and_removes_invalid_dependency_closure() {
    let project = fixture();
    queue(
        project.path(),
        &(task("keep", "")
            + &task("broken", "missing")
            + &task("affected", "broken")
            + &task("cycle-one", "cycle-two")
            + &task("cycle-two", "cycle-one")),
    );
    let (_, preview) = run(project.path(), &["--salvage"]);
    for id in ["broken", "affected", "cycle-one", "cycle-two"] {
        assert!(preview.to_string().contains(id), "{preview}");
    }
    let (ok, applied) = approve(project.path(), &["--salvage"], &preview);
    assert!(ok, "{applied}");
    let repaired = task_queue::parse(
        &fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue"),
    )
    .expect("valid");
    assert_eq!(repaired.tasks.len(), 1);
    assert_eq!(repaired.tasks[0].id, "keep");
}

#[test]
fn changed_input_invalidates_approval_without_creating_backup() {
    let project = fixture();
    let path = project.path().join("src/TODOS.yaml");
    fs::write(&path, "broken YAML [").expect("broken");
    let (_, preview) = run(project.path(), &["--reset", "src/TODOS.yaml"]);
    fs::write(&path, "different broken YAML [").expect("changed");
    let (ok, report) = approve(project.path(), &["--reset", "src/TODOS.yaml"], &preview);
    assert!(!ok);
    assert!(report.to_string().contains("approval"), "{report}");
    assert!(!project.path().join(".kvist-recovery").exists());
    assert_eq!(
        fs::read_to_string(&path).expect("retained"),
        "different broken YAML ["
    );
}

#[test]
fn dry_run_with_approval_still_writes_nothing() {
    let project = fixture();
    fs::write(project.path().join("VISION.md"), b"\xff\xfe").expect("non UTF-8");
    let (_, preview) = run(project.path(), &["--reset", "VISION.md"]);
    let (ok, _) = approve(
        project.path(),
        &["--reset", "VISION.md", "--dry-run"],
        &preview,
    );
    assert!(!ok);
    assert_eq!(
        fs::read(project.path().join("VISION.md")).expect("original"),
        b"\xff\xfe"
    );
    assert!(!project.path().join(".kvist-recovery").exists());
}

#[test]
fn clean_slate_recovers_broken_configuration_and_nested_artifacts() {
    let project = fixture();
    fs::create_dir(project.path().join("nested")).expect("nested");
    fs::write(project.path().join("nested/TODOS.yaml"), "[broken").expect("bad queue");
    fs::write(project.path().join("kvist.toml"), "invalid [").expect("bad config");
    fs::write(project.path().join("src/code.rs"), "preserve source").expect("source");
    fs::create_dir(project.path().join("src/.kvist-attempts")).expect("attempt directory");
    fs::write(
        project.path().join("src/.kvist-attempts/broken.jsonl"),
        "unreadable evidence",
    )
    .expect("evidence");
    let (_, preview) = run(project.path(), &["--clean-slate"]);
    assert!(preview.to_string().contains("effects"), "{preview}");
    let (ok, report) = approve(project.path(), &["--clean-slate"], &preview);
    assert!(ok, "{report}");
    assert_eq!(
        fs::read_to_string(project.path().join("src/code.rs")).expect("source"),
        "preserve source"
    );
    for component in ["src", "nested"] {
        task_queue::parse(
            &fs::read_to_string(project.path().join(component).join("TODOS.yaml")).expect("queue"),
        )
        .expect("strictly valid queue");
    }
    let backup = Path::new(report["backup"].as_str().expect("backup"));
    assert_eq!(
        fs::read_to_string(backup.join("originals/src/.kvist-attempts/broken.jsonl"))
            .expect("preserved evidence"),
        "unreadable evidence"
    );
    let (ok, second) = run(project.path(), &[]);
    assert!(ok, "{second}");
    assert!(!second.to_string().contains("originals/src"));
}

#[test]
fn missing_artifacts_and_wrong_type_objects_have_approved_reset() {
    let project = fixture();
    fs::remove_file(project.path().join("VISION.md")).expect("remove fixture");
    let contract = project.path().join("src/CONTRACT.md");
    fs::remove_file(&contract).expect("remove fixture");
    fs::create_dir(&contract).expect("wrong type");
    fs::write(contract.join("valuable"), "preserve directory contents").expect("valuable");
    let options = ["--reset", "VISION.md", "--reset", "src/CONTRACT.md"];
    let (_, preview) = run(project.path(), &options);
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(ok, "{report}");
    let backup = Path::new(report["backup"].as_str().expect("backup"));
    assert_eq!(
        fs::read_to_string(backup.join("originals/src/CONTRACT.md/valuable")).expect("backup"),
        "preserve directory contents"
    );
    assert!(project.path().join("VISION.md").is_file());
    assert!(contract.is_file());
}

#[test]
fn queue_reset_cannot_orphan_evidence_and_no_source_path_can_be_reset() {
    let project = fixture();
    fs::create_dir(project.path().join("src/.kvist-attempts")).expect("evidence");
    fs::write(project.path().join("src/TODOS.yaml"), "broken").expect("queue");
    let (ok, report) = run(project.path(), &["--reset", "src/TODOS.yaml"]);
    assert!(!ok);
    assert!(report.to_string().contains("clean-slate"), "{report}");
    for path in ["../VISION.md", "/tmp/VISION.md", "src/main.rs"] {
        let (ok, report) = run(project.path(), &["--reset", path]);
        assert!(!ok, "{path}: {report}");
    }
    assert!(!project.path().join(".kvist-recovery").exists());
}

#[cfg(unix)]
#[test]
fn link_reset_archives_the_link_without_touching_its_target_or_following_parents() {
    use std::os::unix::fs::symlink;
    let project = fixture();
    let outside = TempDir::new().expect("outside");
    let target = outside.path().join("private");
    fs::write(&target, "do not touch").expect("target");
    fs::remove_file(project.path().join("VISION.md")).expect("fixture");
    symlink(&target, project.path().join("VISION.md")).expect("link");
    let options = ["--reset", "VISION.md"];
    let (_, preview) = run(project.path(), &options);
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(ok, "{report}");
    assert_eq!(fs::read_to_string(&target).expect("target"), "do not touch");
    let backup = Path::new(report["backup"].as_str().expect("backup"));
    assert_eq!(
        fs::read_link(backup.join("originals/VISION.md")).expect("saved link"),
        target
    );
    symlink(outside.path(), project.path().join("linked")).expect("parent link");
    let (ok, _) = run(project.path(), &["--reset", "linked/CONTRACT.md"]);
    assert!(!ok);
}

#[test]
fn malformed_individual_tasks_do_not_destroy_valid_siblings() {
    let project = fixture();
    let malformed = task("broken", "").replace("kind: test", "kind: unsupported");
    let unknown = task("unknown-field", "")
        .replace("    kind:", "    unexpected: retain in backup\n    kind:");
    let missing_completion = task("no-completion", "")
        .replace("completed_at: 2026-10-29T10:00:00Z", "completed_at: null");
    queue(
        project.path(),
        &(task("keep", "") + &malformed + &unknown + &missing_completion),
    );
    let (_, preview) = run(project.path(), &["--salvage"]);
    assert!(preview.to_string().contains("unexpected"));
    let (ok, report) = approve(project.path(), &["--salvage"], &preview);
    assert!(ok, "{report}");
    let content = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue");
    let repaired = task_queue::parse(&content).expect("valid");
    assert_eq!(
        repaired
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        ["keep", "unknown-field", "no-completion"]
    );
    assert_eq!(repaired.tasks[2].status, task_queue::TaskStatus::Blocked);
    assert!(repaired.tasks[2].timestamps.recovered);
}

#[test]
fn every_semantic_error_family_has_salvage_or_reset_without_weakening_validation() {
    let project = fixture();
    let original = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue");
    let corruptions = [
        ("bad-id", task("bad_id", "")),
        (
            "blank-title",
            task("blank-title", "").replace("title: Test blank-title", "title: ''"),
        ),
        (
            "bad-text",
            task("bad-text", "").replace("context: Recovery fixture.", "context: ''"),
        ),
        (
            "requirement",
            task("requirement", "").replace("REQUIREMENTS.md#Purpose", "no-locator"),
        ),
        (
            "calendar",
            task("calendar", "").replace("2026-10-29", "2026-02-31"),
        ),
        ("self-edge", task("self-edge", "self-edge")),
        (
            "lifecycle",
            task("lifecycle", "").replace("kind: test", "kind: implementation"),
        ),
        ("duplicate", task("duplicate", "") + &task("duplicate", "")),
        ("metadata", task("metadata", "")),
    ];
    for (name, broken) in corruptions {
        fs::write(project.path().join("src/TODOS.yaml"), &original).expect("reset fixture");
        queue(project.path(), &(task("keep", "") + &broken));
        if name == "metadata" {
            let path = project.path().join("src/TODOS.yaml");
            let content = fs::read_to_string(&path).expect("queue");
            fs::write(
                &path,
                content.replace(
                    "requirements_revision: sha256:",
                    "requirements_revision: invalid:",
                ),
            )
            .expect("metadata");
        }
        let (_, preview) = run(project.path(), &["--salvage"]);
        let (ok, report) = approve(project.path(), &["--salvage"], &preview);
        assert!(ok, "{name}: {report}");
        let content = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue");
        let parsed = task_queue::parse(&content).expect("strict validation still applies");
        assert!(parsed.tasks.iter().any(|task| task.id == "keep"), "{name}");
    }
}

#[test]
fn unsupported_versions_are_not_migrated_and_reset_does_not_inherit_acceptance() {
    let project = fixture();
    let path = project.path().join("src/TODOS.yaml");
    let content = fs::read_to_string(&path).expect("queue");
    fs::write(
        &path,
        content.replace("schema_version: 1", "schema_version: 999"),
    )
    .expect("unsupported");
    let (ok, report) = run(project.path(), &["--salvage"]);
    assert!(!ok);
    assert!(report["approval"].is_null());
    let options = ["--reset", "src/TODOS.yaml"];
    let (_, preview) = run(project.path(), &options);
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(ok, "{report}");
    let parsed = task_queue::parse(&fs::read_to_string(path).expect("queue")).expect("valid");
    assert!(parsed.tasks.is_empty());
    assert_eq!(
        parsed.component.revalidation.state,
        task_queue::RevalidationState::Stale
    );
}

#[test]
fn all_native_artifact_names_have_reset_fallback() {
    for name in [
        "kvist.toml",
        "VISION.md",
        "ARCHITECTURE.md",
        "ROOT_CONTRACT.md",
        "src/REQUIREMENTS.md",
        "src/CONTRACT.md",
        "src/DESIGN.md",
        "src/TODOS.yaml",
        "src/IMPL.md",
    ] {
        let project = fixture();
        fs::write(project.path().join(name), b"\xff\nbroken").expect("broken");
        let options = ["--reset", name];
        let (_, preview) = run(project.path(), &options);
        let (ok, report) = approve(project.path(), &options, &preview);
        assert!(ok, "{name}: {report}");
        let retained = Path::new(report["backup"].as_str().expect("backup"))
            .join("originals")
            .join(name);
        assert_eq!(fs::read(retained).expect("original"), b"\xff\nbroken");
    }
}

#[test]
fn markdown_reset_cannot_bypass_an_active_component_task() {
    let project = fixture();
    queue(
        project.path(),
        &task("active-task", "")
            .replace("status: completed", "status: in-progress")
            .replace("completed_at: 2026-10-29T10:00:00Z", "completed_at: null"),
    );
    let path = project.path().join("src/REQUIREMENTS.md");
    fs::write(&path, "retain broken intent").expect("intent");
    let options = ["--reset", "src/REQUIREMENTS.md"];
    let (_, preview) = run(project.path(), &options);
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(!ok, "{report}");
    assert!(report.to_string().contains("active/fenced"), "{report}");
    assert_eq!(
        fs::read_to_string(path).expect("unchanged"),
        "retain broken intent"
    );
    assert!(!project.path().join(".kvist-recovery").exists());
}

#[test]
fn aggregate_text_limit_accepts_exactly_64_mib_and_rejects_an_additional_queue() {
    let project = fixture();
    let original = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue");
    let markdown_bytes = [
        "VISION.md",
        "ARCHITECTURE.md",
        "ROOT_CONTRACT.md",
        "src/REQUIREMENTS.md",
        "src/CONTRACT.md",
        "src/DESIGN.md",
        "src/IMPL.md",
    ]
    .iter()
    .map(|path| {
        fs::read(project.path().join(path))
            .expect("native Markdown")
            .len()
    })
    .sum::<usize>();
    let padded = format!(
        "{original}#{}\n",
        "x".repeat(1024 * 1024 - original.len() - 2)
    );
    assert_eq!(padded.len(), 1024 * 1024);
    let root_padded = format!(
        "{original}#{}\n",
        "x".repeat(1024 * 1024 - markdown_bytes - original.len() - 2)
    );
    assert_eq!(
        root_padded.len() + markdown_bytes + 63 * padded.len(),
        64 * 1024 * 1024
    );
    fs::write(project.path().join("src/TODOS.yaml"), &root_padded).expect("padded queue");
    for index in 0..63 {
        let directory = project.path().join(format!("src/bundle-{index}"));
        fs::create_dir(&directory).expect("bundle");
        fs::write(directory.join("TODOS.yaml"), &padded).expect("padded queue");
    }
    let (_, report) = run(project.path(), &["--dry-run"]);
    assert!(report["error"].is_null(), "{report}");
    assert_eq!(report["queues"].as_array().expect("queues").len(), 64);
    let directory = project.path().join("src/over-limit");
    fs::create_dir(&directory).expect("bundle");
    fs::write(directory.join("TODOS.yaml"), original).expect("additional queue");
    let (ok, report) = run(project.path(), &["--dry-run"]);
    assert!(!ok, "{report}");
    assert!(
        report["error"].as_str().expect("error").contains("64 MiB"),
        "{report}"
    );
    assert!(!project.path().join(".kvist-recovery").exists());
}

#[test]
fn oversized_artifacts_can_be_backed_up_and_reset_without_parsing() {
    let project = fixture();
    let bytes = vec![b'x'; 1024 * 1024 + 1];
    fs::write(project.path().join("src/TODOS.yaml"), &bytes).expect("oversized");
    let options = ["--reset", "src/TODOS.yaml"];
    let (_, preview) = run(project.path(), &options);
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(ok, "{report}");
    assert_eq!(
        fs::read(
            Path::new(report["backup"].as_str().expect("backup")).join("originals/src/TODOS.yaml")
        )
        .expect("backup"),
        bytes
    );
}

#[test]
fn backup_failure_prevents_all_mutation() {
    let project = fixture();
    fs::write(project.path().join("src/TODOS.yaml"), "[broken").expect("broken");
    fs::write(project.path().join(".kvist-recovery"), "conflict").expect("storage conflict");
    let options = ["--reset", "src/TODOS.yaml"];
    let (_, preview) = run(project.path(), &options);
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(!ok);
    assert!(report.to_string().contains("private directory"), "{report}");
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("unchanged"),
        "[broken"
    );
    assert_eq!(
        fs::read_to_string(project.path().join(".kvist-recovery")).expect("conflict"),
        "conflict"
    );
}

#[test]
fn alternate_backup_directory_recovers_without_manually_moving_conflicting_storage() {
    let project = fixture();
    let external = TempDir::new().expect("external storage");
    let backup_dir = external.path().join("private-backups");
    fs::write(project.path().join(".kvist-recovery"), "retain conflict").expect("conflict");
    fs::write(project.path().join("src/TODOS.yaml"), "[broken").expect("queue");
    let options = [
        "--reset",
        "src/TODOS.yaml",
        "--backup-dir",
        backup_dir.to_str().expect("path"),
    ];
    let (_, preview) = run(project.path(), &options);
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(ok, "{report}");
    assert!(Path::new(report["backup"].as_str().expect("backup")).starts_with(&backup_dir));
    assert_eq!(
        fs::read_to_string(project.path().join(".kvist-recovery")).expect("conflict"),
        "retain conflict"
    );
}

#[test]
fn in_project_alternative_backups_are_excluded_from_git_and_component_discovery() {
    let project = fixture();
    assert!(
        Command::new("git")
            .arg("init")
            .arg("--quiet")
            .arg(project.path())
            .status()
            .expect("git")
            .success()
    );
    fs::write(project.path().join(".kvist-recovery"), "retain conflict").expect("conflict");
    fs::write(project.path().join("src/TODOS.yaml"), "[broken").expect("queue");
    let options = [
        "--reset",
        "src/TODOS.yaml",
        "--backup-dir",
        ".kvist-recovery-alternative",
    ];
    let (_, preview) = run(project.path(), &options);
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(ok, "{report}");
    let output = Command::new("git")
        .arg("-C")
        .arg(project.path())
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            ".kvist-recovery-alternative",
        ])
        .output()
        .expect("git status");
    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "backup must not be staged by git add ."
    );
    let (ok, report) = run(
        project.path(),
        &["--backup-dir", ".kvist-recovery-alternative"],
    );
    assert!(ok, "{report}");
    assert!(!report.to_string().contains("originals/src"));
}

#[test]
fn syntactically_broken_task_fragments_can_be_quarantined_without_losing_valid_siblings() {
    let project = fixture();
    queue(
        project.path(),
        &(task("keep", "") + "  - id: broken\n    title: [unclosed\n" + &task("after", "")),
    );
    let (_, preview) = run(project.path(), &["--salvage"]);
    assert!(preview["approval"].is_string(), "{preview}");
    let (ok, report) = approve(project.path(), &["--salvage"], &preview);
    assert!(ok, "{report}");
    let parsed = task_queue::parse(
        &fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue"),
    )
    .expect("valid");
    assert_eq!(
        parsed
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        ["keep", "after"]
    );
}

#[test]
fn clean_slate_quarantines_an_obstructing_component_parent() {
    let project = fixture();
    // Keep source in src, and configure a separate invalid component root.
    let config = fs::read_to_string(project.path().join("kvist.toml")).expect("config");
    fs::write(
        project.path().join("kvist.toml"),
        config.replace(
            "component_root = \"src\"",
            "component_root = \"obstructed\"",
        ),
    )
    .expect("config");
    fs::write(project.path().join("obstructed"), "preserve this object").expect("obstruction");
    let (_, preview) = run(project.path(), &["--clean-slate"]);
    let (ok, report) = approve(project.path(), &["--clean-slate"], &preview);
    assert!(ok, "{report}");
    assert!(project.path().join("obstructed/TODOS.yaml").is_file());
    assert_eq!(
        fs::read_to_string(
            Path::new(report["backup"].as_str().expect("backup")).join("originals/obstructed")
        )
        .expect("quarantine"),
        "preserve this object"
    );
}

#[test]
fn component_bundles_inside_state_directories_are_not_mistaken_for_disposable_runtime_state() {
    let project = fixture();
    fs::rename(project.path().join("src"), project.path().join(".kvist"))
        .expect("converted component");
    let config = fs::read_to_string(project.path().join("kvist.toml")).expect("config");
    fs::write(
        project.path().join("kvist.toml"),
        config.replace("component_root = \"src\"", "component_root = \".kvist\""),
    )
    .expect("config");
    fs::write(
        project.path().join(".kvist/code.rs"),
        "preserve converted source",
    )
    .expect("source");
    fs::write(
        project.path().join(".kvist/rust-toolchain.json"),
        "[broken manifest",
    )
    .expect("manifest");
    let (_, preview) = run(project.path(), &["--clean-slate"]);
    let (ok, report) = approve(project.path(), &["--clean-slate"], &preview);
    assert!(ok, "{report}");
    assert_eq!(
        fs::read_to_string(project.path().join(".kvist/code.rs")).expect("source"),
        "preserve converted source"
    );
    assert!(!project.path().join(".kvist/rust-toolchain.json").exists());
    assert!(
        Path::new(report["backup"].as_str().expect("backup"))
            .join("originals/.kvist/rust-toolchain.json")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn backups_are_private_and_retain_non_utf8_bytes_and_original_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let project = fixture();
    let path = project.path().join("VISION.md");
    fs::write(&path, b"\xffbroken").expect("bytes");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).expect("mode");
    let options = ["--reset", "VISION.md"];
    let (_, preview) = run(project.path(), &options);
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(ok, "{report}");
    let bundle = Path::new(report["backup"].as_str().expect("backup"));
    assert_eq!(
        fs::metadata(bundle).expect("bundle").permissions().mode() & 0o077,
        0
    );
    let saved = bundle.join("originals/VISION.md");
    assert_eq!(
        fs::metadata(&saved).expect("mode").permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::read(saved).expect("bytes"), b"\xffbroken");
    let manifest: Value =
        serde_json::from_slice(&fs::read(bundle.join("manifest.json")).expect("manifest"))
            .expect("JSON");
    assert_eq!(manifest["state"], "complete");
    assert_eq!(manifest["applied"][0], "VISION.md");
}

#[test]
fn configured_components_inside_state_namespaces_are_fully_inventoried() {
    let project = fixture();
    let component = project.path().join(".kvist/components");
    fs::create_dir_all(component.join("child")).expect("component tree");
    for name in [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        fs::copy(project.path().join("src").join(name), component.join(name))
            .expect("component artifact");
    }
    let config = fs::read_to_string(project.path().join("kvist.toml")).expect("config");
    fs::write(
        project.path().join("kvist.toml"),
        config.replace(
            "component_root = \"src\"",
            "component_root = \".kvist/components\"",
        ),
    )
    .expect("configuration");
    fs::write(component.join("child/TODOS.yaml"), "[broken").expect("child");
    let (ok, report) = run(project.path(), &[]);
    assert!(!ok, "nested invalidity must not be hidden: {report}");
    assert!(
        report
            .to_string()
            .contains(".kvist/components/child/TODOS.yaml"),
        "{report}"
    );
    let (_, preview) = run(project.path(), &["--clean-slate"]);
    assert!(
        preview
            .to_string()
            .contains(".kvist/components/child/TODOS.yaml"),
        "{preview}"
    );
    let (ok, report) = approve(project.path(), &["--clean-slate"], &preview);
    assert!(ok, "{report}");
    // Namespace components must remain visible after configuration resets to ".".
    fs::write(component.join("child/TODOS.yaml"), "[broken again").expect("child");
    let (ok, report) = run(project.path(), &[]);
    assert!(!ok, "{report}");
    assert!(
        report
            .to_string()
            .contains(".kvist/components/child/TODOS.yaml")
    );
}

#[test]
fn reset_configuration_can_select_a_component_root_without_manual_toml_edits() {
    let project = fixture();
    fs::write(project.path().join("kvist.toml"), "[broken").expect("config");
    let options = ["--reset", "kvist.toml", "--component-root", "src"];
    let (_, preview) = run(project.path(), &options);
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(ok, "{report}");
    assert_eq!(
        kvist::config::load(project.path())
            .expect("config")
            .component_root,
        Path::new("src")
    );
    for invalid in ["../outside", "/tmp", ".git", ".kvist-recovery"] {
        let (ok, report) = run(
            project.path(),
            &["--reset", "kvist.toml", "--component-root", invalid],
        );
        assert!(!ok, "{report}");
    }
}

#[test]
fn root_component_paths_are_normalized_to_one_operation_per_object() {
    let project = fixture();
    for name in [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        fs::copy(
            project.path().join("src").join(name),
            project.path().join(name),
        )
        .expect("root component");
    }
    let config = fs::read_to_string(project.path().join("kvist.toml")).expect("config");
    fs::write(
        project.path().join("kvist.toml"),
        config.replace("component_root = \"src\"", "component_root = \".\""),
    )
    .expect("config");
    fs::write(project.path().join("TODOS.yaml"), "[broken").expect("root queue");
    let options = ["--reset", "./TODOS.yaml"];
    let (_, preview) = run(project.path(), &options);
    assert_eq!(
        preview["plan"].as_array().expect("plan").len(),
        1,
        "{preview}"
    );
    let (ok, report) = approve(project.path(), &options, &preview);
    assert!(ok, "{report}");
}
