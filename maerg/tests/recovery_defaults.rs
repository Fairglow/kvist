use std::{fs, path::Path, process::Command};

use kvist::{
    component_documents::{self, DocumentKind},
    init::initialize,
    project_state, task_commands, task_queue,
};
use serde_json::Value;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    project
}

fn repair(project: &Path, args: &[&str]) -> (bool, Value) {
    let state = TempDir::new().expect("private user state");
    let output = Command::new(env!("CARGO_BIN_EXE_kvist"))
        .env("HOME", state.path())
        .env("XDG_CONFIG_HOME", state.path())
        .env("XDG_STATE_HOME", state.path())
        .args(["--json", "repair"])
        .arg(project)
        .args(args)
        .output()
        .expect("repair");
    let bytes = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let report = serde_json::from_slice(bytes)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(bytes)));
    (output.status.success(), report)
}

fn approve(project: &Path, preview: &Value) -> Value {
    let (ok, report) = repair(
        project,
        &["--approve", preview["approval"].as_str().expect("token")],
    );
    assert!(ok, "{report}");
    report
}

fn task(id: &str, dependency: &str, timestamps: &str) -> String {
    format!(
        r#"  - id: {id}
    title: Preserve {id}
    description: Preserve useful authored work.
    context: Recovery regression.
    purpose: Preserve data without inventing success.
    expected_outcome: A valid recovery record.
    kind: test
    status: completed
    depends_on: [{dependency}]
    requirements: [REQUIREMENTS.md#Purpose]
{timestamps}    blocked_reason: null
    recovery_state: null
"#
    )
}

const TIMESTAMPS: &str = "    timestamps:\n      created_at: 2026-10-29T10:00:00Z\n      updated_at: 2026-10-29T10:00:00Z\n      completed_at: 2026-10-29T10:00:00Z\n";

fn write_tasks(project: &Path, tasks: &str) -> String {
    let path = project.join("src/TODOS.yaml");
    let original = fs::read_to_string(&path).expect("queue");
    let header = original.split("tasks:").next().expect("header");
    let contents = format!("{header}tasks:\n{tasks}");
    fs::write(path, &contents).expect("tasks");
    contents
}

fn document(kind: DocumentKind, first_body: &str) -> String {
    let template = match kind {
        DocumentKind::Requirements => component_documents::COMPONENT_REQUIREMENTS_TEMPLATE,
        DocumentKind::Contract => component_documents::COMPONENT_CONTRACT_TEMPLATE,
        DocumentKind::Design => component_documents::COMPONENT_DESIGN_TEMPLATE,
    };
    let mut contents = format!("{}\n# Fixture\n", template.lines().next().expect("marker"));
    for (index, heading) in template
        .lines()
        .filter(|line| line.starts_with("## "))
        .enumerate()
    {
        contents.push_str(&format!(
            "\n{heading}\n\n{}\n",
            if index == 0 {
                first_body
            } else {
                "Reviewed fixture content."
            }
        ));
    }
    contents
}

fn track(project: &Path) {
    for args in [&["init", "--quiet"][..], &["add", "."][..]] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(project)
                .status()
                .expect("track fixtures")
                .success()
        );
    }
}

#[test]
fn missing_timestamp_metadata_is_repaired_without_changing_task_state_or_authority() {
    for timestamps in [
        String::new(),
        "    timestamps: null\n".to_owned(),
        "    timestamps: {}\n".to_owned(),
        TIMESTAMPS.replace("      created_at: 2026-10-29T10:00:00Z\n", ""),
        TIMESTAMPS.replace("      updated_at: 2026-10-29T10:00:00Z\n", ""),
        TIMESTAMPS.replace("      completed_at: 2026-10-29T10:00:00Z\n", ""),
        TIMESTAMPS.replace("completed_at: 2026-10-29T10:00:00Z", "completed_at: null"),
        TIMESTAMPS
            .replace("      created_at: 2026-10-29T10:00:00Z\n", "")
            .replace("2026-10-29", "1950-10-29"),
    ] {
        let project = fixture();
        fs::create_dir(project.path().join("src/.kvist-attempts")).expect("empty evidence storage");
        let original = write_tasks(
            project.path(),
            &(task("retain", "", &timestamps)
                .replace(
                    "    recovery_state: null",
                    "    recovery_state: null\n    acceptance_id: recorded-reference\n    disposition: accept",
                )
                + &task("dependant", "retain", TIMESTAMPS)),
        );
        let path = project.path().join("src/TODOS.yaml");
        let before: serde_yaml::Value = serde_yaml::from_str(&original).expect("original YAML");
        assert!(
            project_state::inspect(project.path())
                .expect("inspection")
                .guidance
                .contains("kvist repair")
        );
        let state = TempDir::new().expect("private user state");
        let status = Command::new(env!("CARGO_BIN_EXE_kvist"))
            .env("HOME", state.path())
            .env("XDG_CONFIG_HOME", state.path())
            .env("XDG_STATE_HOME", state.path())
            .current_dir(project.path())
            .arg("status")
            .output()
            .expect("status");
        let guidance = String::from_utf8_lossy(&status.stdout);
        assert!(guidance.contains("Run 'kvist repair'"), "{guidance}");
        assert!(!guidance.contains("repair --salvage"), "{guidance}");
        let (ok, preview) = repair(project.path(), &["--dry-run"]);
        assert!(!ok, "{preview}");
        assert_eq!(preview["plan"][0]["requires_approval"], false, "{preview}");
        assert_eq!(fs::read_to_string(&path).expect("unchanged"), original);
        assert!(!project.path().join(".kvist-recovery").exists());
        let (ok, report) = repair(project.path(), &[]);
        assert!(
            ok,
            "missing metadata needs no destructive approval: {report}"
        );
        assert_eq!(report["plan"][0]["kind"], "normalize");
        assert_eq!(report["plan"][0]["requires_approval"], false);
        assert!(report["blockers"].as_array().expect("blockers").is_empty());
        let contents = fs::read_to_string(&path).expect("repaired");
        let recovered = task_queue::parse(&contents).expect("strict native validity");
        let after: serde_yaml::Value = serde_yaml::from_str(&contents).expect("repaired YAML");
        assert_eq!(before["component"], after["component"]);
        let mut original_task = before["tasks"][0].clone();
        let mut repaired_task = after["tasks"][0].clone();
        original_task
            .as_mapping_mut()
            .expect("task")
            .remove("timestamps");
        repaired_task
            .as_mapping_mut()
            .expect("task")
            .remove("timestamps");
        assert_eq!(
            original_task, repaired_task,
            "only timestamp metadata may change"
        );
        assert_eq!(recovered.tasks[0].status, task_queue::TaskStatus::Completed);
        assert!(recovered.tasks[0].timestamps.recovered);
        for field in ["created_at", "updated_at", "completed_at"] {
            if !before["tasks"][0]["timestamps"][field].is_null() {
                assert_eq!(
                    before["tasks"][0]["timestamps"][field], after["tasks"][0]["timestamps"][field],
                    "{field}"
                );
            }
        }
        assert_eq!(before["tasks"][1], after["tasks"][1]);
        let backup = Path::new(report["backup"].as_str().expect("backup"));
        assert_eq!(
            fs::read_to_string(backup.join("originals/src/TODOS.yaml")).expect("backup"),
            original
        );
        let (ok, repeat) = repair(project.path(), &[]);
        assert!(ok, "{repeat}");
        assert!(repeat["plan"].as_array().expect("plan").is_empty());
        assert_eq!(fs::read_to_string(&path).expect("idempotent"), contents);
    }
}

#[test]
fn missing_dates_preserve_pending_and_blocked_states_and_existing_blockers() {
    for status in ["pending", "blocked", "awaiting-decision"] {
        let project = fixture();
        let reason = if status == "pending" {
            "null"
        } else {
            "\"Original human decision\""
        };
        write_tasks(
            project.path(),
            &task("retain", "", "")
                .replace("status: completed", &format!("status: {status}"))
                .replace("blocked_reason: null", &format!("blocked_reason: {reason}")),
        );
        let (ok, report) = repair(project.path(), &[]);
        assert!(ok, "{report}");
        let text = fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("repaired");
        let queue: serde_yaml::Value = serde_yaml::from_str(&text).expect("queue");
        assert_eq!(queue["tasks"][0]["status"].as_str(), Some(status));
        if status != "pending" {
            assert_eq!(
                queue["tasks"][0]["blocked_reason"].as_str(),
                Some("Original human decision")
            );
        }
    }
}

#[test]
fn missing_metadata_does_not_bypass_unresolved_execution_evidence() {
    let project = fixture();
    let original = write_tasks(project.path(), &task("retain", "", ""));
    fs::create_dir(project.path().join("src/.kvist-attempts")).expect("evidence");
    for evidence in ["unreadable evidence\n", "{\"schema_version\":1}\n"] {
        fs::write(
            project.path().join("src/.kvist-attempts/retain.jsonl"),
            evidence,
        )
        .expect("unresolved");
        let (ok, report) = repair(project.path(), &[]);
        assert!(!ok, "{report}");
        assert_eq!(
            fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("unchanged"),
            original
        );
        assert!(!project.path().join(".kvist-recovery").exists());
    }
}

#[test]
fn metadata_repair_waits_for_approval_of_other_destructive_operations() {
    let project = fixture();
    let original = write_tasks(project.path(), &task("retain", "", ""));
    fs::write(project.path().join("VISION.md"), "Preserve draft intent.\n").expect("broken");
    let (ok, report) = repair(project.path(), &[]);
    assert!(!ok, "{report}");
    assert!(
        report["plan"]
            .as_array()
            .expect("plan")
            .iter()
            .any(|op| op["requires_approval"] == true)
    );
    assert_eq!(
        fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("unchanged"),
        original
    );
    assert!(!project.path().join(".kvist-recovery").exists());
}

#[test]
fn malformed_timestamp_history_requires_approved_blocked_recovery() {
    for timestamps in [
        "    timestamps: malformed-metadata\n".to_owned(),
        TIMESTAMPS.replace("2026-10-29T10:00:00Z", "not-a-date"),
    ] {
        let project = fixture();
        let original = write_tasks(
            project.path(),
            &(task("retain", "", &timestamps) + &task("dependant", "retain", TIMESTAMPS)),
        );
        let path = project.path().join("src/TODOS.yaml");
        let guidance = kvist::repair::repair(project.path(), false)
            .expect("default proposal")
            .to_string();
        assert!(!guidance.contains("use --salvage"), "{guidance}");
        assert!(
            guidance.contains("salvage planning is the default")
                && guidance.contains("--ai PROFILE --proposal PATH"),
            "{guidance}"
        );
        let (ok, preview) = repair(project.path(), &[]);
        assert!(!ok, "{preview}");
        assert!(preview["approval"].is_string(), "{preview}");
        assert!(
            !preview["plan"].to_string().contains("remove task"),
            "{preview}"
        );
        assert!(
            !preview["plan"]
                .to_string()
                .contains("preserve retained task statuses"),
            "generic reporting must not contradict explicit completed -> blocked conversion: {preview}"
        );
        assert_eq!(fs::read_to_string(&path).expect("original"), original);
        assert!(!project.path().join(".kvist-recovery").exists());
        let report = approve(project.path(), &preview);
        let contents = fs::read_to_string(&path).expect("recovered");
        let recovered = task_queue::parse(&contents).expect("strict native validity");
        assert_eq!(recovered.tasks.len(), 2);
        assert_eq!(recovered.tasks[0].id, "retain");
        assert_eq!(recovered.tasks[0].status, task_queue::TaskStatus::Blocked);
        assert!(recovered.tasks[0].timestamps.completed_at.is_none());
        assert_eq!(recovered.tasks[1].depends_on, ["retain"]);
        let yaml: serde_yaml::Value = serde_yaml::from_str(&contents).expect("native YAML");
        assert_eq!(
            yaml["tasks"][0]["timestamps"]["recovered"].as_bool(),
            Some(true)
        );
        let reason = recovered.tasks[0]
            .blocked_reason
            .as_deref()
            .expect("blocker");
        assert!(
            reason.contains("completed") && reason.contains("unknown"),
            "{reason}"
        );
        assert!(!report["blockers"].as_array().expect("blockers").is_empty());
        let backup = Path::new(report["backup"].as_str().expect("backup"));
        assert_eq!(
            fs::read_to_string(backup.join("originals/src/TODOS.yaml")).expect("backup"),
            original
        );
        assert!(repair(project.path(), &[]).0);
    }
}

#[test]
fn default_lossless_normalization_still_applies_without_approval() {
    let project = fixture();
    write_tasks(
        project.path(),
        &(task("later", "earlier", TIMESTAMPS) + &task("earlier", "", TIMESTAMPS)),
    );
    let (ok, report) = repair(project.path(), &[]);
    assert!(ok, "{report}");
    assert_eq!(report["plan"][0]["kind"], "normalize");
    assert_eq!(report["plan"][0]["requires_approval"], false);
    let queue = task_queue::parse(
        &fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue"),
    )
    .expect("valid");
    assert_eq!(queue.tasks[0].id, "earlier");
    assert_eq!(queue.tasks[0].status, task_queue::TaskStatus::Completed);
    assert!(report["backup"].is_string());
}

#[test]
fn nested_subsections_are_content_and_fenced_headings_are_not_sections() {
    let kind = DocumentKind::Design;
    let nested = document(
        kind,
        "### Details\n\nReal design body.\n\n```rust\nfn main() {}\n```",
    );
    assert!(component_documents::validate(kind, &nested).is_valid());
    let fenced = nested.replace("## Design overview", "```markdown\n## Design overview\n```");
    assert!(!component_documents::validate(kind, &fenced).is_valid());
    let project = fixture();
    let path = project.path().join("src/DESIGN.md");
    fs::write(&path, &nested).expect("design");
    let (ok, report) = repair(project.path(), &[]);
    assert!(ok, "{report}");
    assert_eq!(fs::read_to_string(&path).expect("preserved"), nested);
}

#[test]
fn fenced_sections_are_preserved_and_real_empty_sections_become_valid_blocked_drafts() {
    let project = fixture();
    let kind = DocumentKind::Design;
    let original = document(kind, "").replace(
        "## Internal structure",
        "## Internal structure\n\n~~~markdown\n## Design overview\nKeep this example intact.\n~~~",
    );
    let path = project.path().join("src/DESIGN.md");
    fs::write(&path, &original).expect("design");
    let (_, preview) = repair(project.path(), &[]);
    let report = approve(project.path(), &preview);
    let recovered = fs::read_to_string(&path).expect("design");
    assert!(component_documents::validate(kind, &recovered).is_valid());
    assert!(recovered.contains("~~~markdown\n## Design overview\nKeep this example intact.\n~~~"));
    assert!(recovered.contains("<!-- kvist-recovery-blocker:"));
    assert!(!report["blockers"].as_array().expect("blockers").is_empty());
    track(project.path());
    let inspection = project_state::inspect(project.path()).expect("inspect");
    assert_eq!(inspection.components[0].state.name(), "blocked");
    assert!(
        task_commands::accept(project.path(), Path::new("."), false, None, false).is_err(),
        "accept must not clear unresolved intent"
    );
    let error = task_commands::next(project.path(), Path::new(".")).expect_err("blocked intent");
    assert!(
        error.to_string().contains("kvist-recovery-blocker"),
        "{error}"
    );
    assert!(error.to_string().contains("DESIGN.md"), "{error}");
    let resolved = recovered
        .lines()
        .filter(|line| !line.starts_with("<!-- kvist-recovery-blocker:"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, resolved).expect("human resolves draft");
    track(project.path());
    task_commands::accept(project.path(), Path::new("."), false, None, false)
        .expect("explicit acceptance after resolving markers");
}

#[test]
fn marker_examples_in_code_fences_are_inert_but_root_drafts_block_readiness() {
    let project = fixture();
    let design = document(
        DocumentKind::Design,
        "```markdown\n<!-- kvist-recovery-blocker: example only -->\n```",
    );
    fs::write(project.path().join("src/DESIGN.md"), design).expect("example");
    track(project.path());
    task_commands::accept(project.path(), Path::new("."), false, None, false)
        .expect("fenced example is not an active blocker");
    let (_, preview) = repair(project.path(), &["--reset", "ROOT_CONTRACT.md"]);
    let token = preview["approval"].as_str().expect("token");
    let (ok, report) = repair(
        project.path(),
        &["--reset", "ROOT_CONTRACT.md", "--approve", token],
    );
    assert!(ok, "{report}");
    track(project.path());
    assert_eq!(
        project_state::inspect(project.path())
            .expect("inspect")
            .state
            .name(),
        "blocked"
    );
    assert!(task_commands::accept(project.path(), Path::new("."), false, None, false).is_err());
    assert!(task_commands::next(project.path(), Path::new(".")).is_err());
}

#[test]
fn recovered_timestamp_provenance_survives_explicit_resumption() {
    let project = fixture();
    write_tasks(
        project.path(),
        &task("retain", "", "")
            .replace("status: completed", "status: blocked")
            .replace("blocked_reason: null", "blocked_reason: Original blocker"),
    );
    let (ok, report) = repair(project.path(), &[]);
    assert!(ok, "{report}");
    track(project.path());
    task_commands::accept(project.path(), Path::new("."), false, None, false)
        .expect("accept structurally valid blocked queue");
    task_commands::transition(
        project.path(),
        Path::new("."),
        "retain",
        task_queue::TaskStatus::Pending,
        None,
    )
    .expect("explicit resumption");
    let yaml: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(project.path().join("src/TODOS.yaml")).expect("queue"),
    )
    .expect("YAML");
    assert_eq!(
        yaml["tasks"][0]["timestamps"]["recovered"].as_bool(),
        Some(true)
    );
    assert_ne!(
        yaml["tasks"][0]["timestamps"]["updated_at"].as_str(),
        Some("1970-01-01T00:00:00Z")
    );
}

#[test]
fn parent_contract_blockers_cross_namespaces_but_parent_private_intent_does_not() {
    let project = fixture();
    let child = project.path().join("src/namespace/child");
    fs::create_dir_all(&child).expect("namespace");
    for name in [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        fs::copy(project.path().join("src").join(name), child.join(name)).expect("child bundle");
    }
    track(project.path());
    task_commands::accept(
        project.path(),
        Path::new("namespace/child"),
        false,
        None,
        false,
    )
    .expect("record nearest parent contract");
    let parent_design = project.path().join("src/DESIGN.md");
    let original_design = fs::read_to_string(&parent_design).expect("design");
    fs::write(
        &parent_design,
        format!("{original_design}\n<!-- kvist-recovery-blocker: parent private draft -->\n"),
    )
    .expect("parent design blocker");
    track(project.path());
    let inspection = project_state::inspect(project.path()).expect("inspect");
    let inspected_child = inspection
        .components
        .iter()
        .find(|component| component.path == Path::new("namespace/child"))
        .expect("child");
    assert_ne!(
        inspected_child.state.name(),
        "blocked",
        "parent design must not propagate"
    );
    let parent_contract = project.path().join("src/CONTRACT.md");
    let original_contract = fs::read_to_string(&parent_contract).expect("contract");
    fs::write(
        &parent_contract,
        format!(
            "{original_contract}\n<!-- kvist-recovery-blocker: parent public contract draft -->\n"
        ),
    )
    .expect("parent contract blocker");
    track(project.path());
    let inspection = project_state::inspect(project.path()).expect("inspect");
    let inspected_child = inspection
        .components
        .iter()
        .find(|component| component.path == Path::new("namespace/child"))
        .expect("child");
    assert_eq!(inspected_child.state.name(), "blocked");
    assert!(
        inspected_child
            .recovery_blockers
            .iter()
            .any(|path| path == "../../CONTRACT.md")
    );
    assert!(
        task_commands::accept(
            project.path(),
            Path::new("namespace/child"),
            false,
            None,
            false
        )
        .is_err()
    );
    assert!(task_commands::next(project.path(), Path::new("namespace/child")).is_err());
}
