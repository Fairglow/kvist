use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap()
}

fn project() -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    kvist::init::initialize(project.path()).unwrap();
    let root = project.path().join("src");
    let source = fs::read_to_string(root.join("TODOS.yaml")).unwrap();
    let queue = kvist::task_queue::parse(&source).unwrap();
    let child_queue = source.replace(
        "parent_contract: null",
        &format!(
            "parent_contract:\n    path: ../CONTRACT.md\n    revision: {}",
            queue.component.contract_revision
        ),
    );
    for name in [
        "alpha",
        "alpha/child",
        "beta",
        "space child",
        "pipe|child",
        "-flag",
    ] {
        let directory = root.join(name);
        fs::create_dir_all(&directory).unwrap();
        for artifact in ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md", "IMPL.md"] {
            fs::copy(root.join(artifact), directory.join(artifact)).unwrap();
        }
        fs::write(directory.join("TODOS.yaml"), &child_queue).unwrap();
    }
    project
}

fn text(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn project_status_is_one_row_per_component_and_details_are_explicit() {
    let project = project();
    let overview = text(run(project.path(), &["status"]));
    assert!(
        overview.contains("| Component | State | Artifacts | Tasks | Next action |"),
        "{overview}"
    );
    assert_eq!(
        overview
            .lines()
            .filter(|line| line.starts_with("| "))
            .count(),
        9,
        "{overview}"
    );
    assert!(
        overview.contains("| . | current | 5/5 valid | 0/0 completed |"),
        "{overview}"
    );
    assert!(overview.contains("| alpha | current |"), "{overview}");
    assert!(overview.contains("kvist status alpha"), "{overview}");
    assert!(
        overview.contains("| pipe\\|child | current |"),
        "{overview}"
    );
    assert!(!overview.contains("## Component:"), "{overview}");
    assert!(!overview.contains("| VCS artifact |"), "{overview}");
    assert!(!overview.contains("### Changed document:"), "{overview}");
    let detailed = text(run(project.path(), &["status", "--details"]));
    assert_eq!(detailed.matches("## Component:").count(), 7, "{detailed}");
    assert!(
        detailed.contains("**Task progress:** 0/0 completed"),
        "{detailed}"
    );
    assert!(text(run(project.path(), &["doctor"])).contains("## Component: alpha"));
}

#[test]
fn component_selection_is_exact_normalized_and_shared_by_every_format() {
    let project = project();
    for arguments in [vec!["init", "--quiet"], vec!["add", "."]] {
        assert!(
            Command::new("git")
                .args(arguments)
                .current_dir(project.path())
                .status()
                .unwrap()
                .success()
        );
    }
    for argument in [
        "alpha".to_owned(),
        "./alpha".to_owned(),
        "src/alpha".to_owned(),
        project.path().join("src/alpha").display().to_string(),
    ] {
        for flag in [None, Some("--details")] {
            let mut arguments = vec!["status", &argument];
            arguments.extend(flag);
            let output = text(run(project.path(), &arguments));
            assert_eq!(output.matches("## Component:").count(), 1, "{output}");
            assert!(output.contains("## Component: alpha\n"), "{output}");
            assert!(output.contains("| VCS artifact |"), "{output}");
            assert!(!output.contains("src/beta/"), "{output}");
            assert!(!output.contains("src/alpha/child/"), "{output}");
        }
    }
    let native = run(project.path(), &["status", "alpha", "--format", "json"]);
    assert!(native.status.success());
    let value: serde_json::Value = serde_json::from_slice(&native.stdout).unwrap();
    assert_eq!(value["format_version"], 1);
    assert_eq!(value["components"].as_array().unwrap().len(), 1);
    assert_eq!(value["components"][0]["path"], "alpha");
    assert_eq!(
        native.stdout,
        run(project.path(), &["--json", "status", "alpha"]).stdout
    );
    let native_text = text(run(
        project.path(),
        &["status", "alpha", "--format", "text"],
    ));
    assert!(native_text.starts_with("status-format-version: 1\n"));
    assert_eq!(native_text.matches("\ncomponent: ").count(), 1);
    assert!(native_text.contains("\ncomponent: alpha state: current\n"));
    for component in [".", "space child", "pipe|child", "./-flag"] {
        let output = text(run(project.path(), &["status", component]));
        assert_eq!(output.matches("## Component:").count(), 1, "{output}");
    }
}

#[test]
fn dot_component_roots_do_not_leak_peer_artifact_or_vcs_details() {
    let project = project();
    let configuration = project.path().join("kvist.toml");
    let configuration_text = fs::read_to_string(&configuration).unwrap();
    fs::write(
        &configuration,
        configuration_text.replace("component_root = \"src\"", "component_root = \".\""),
    )
    .unwrap();
    for artifact in [
        "REQUIREMENTS.md",
        "CONTRACT.md",
        "DESIGN.md",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        fs::rename(
            project.path().join("src").join(artifact),
            project.path().join(artifact),
        )
        .unwrap();
    }
    let selected = text(run(
        project.path(),
        &["status", "src/alpha", "--format", "json"],
    ));
    let value: serde_json::Value = serde_json::from_str(&selected).unwrap();
    assert_eq!(value["components"].as_array().unwrap().len(), 1);
    assert_eq!(value["components"][0]["path"], "src/alpha");
    assert!(!selected.contains("src/beta/"), "{selected}");
    assert!(!selected.contains("src/alpha/child/"), "{selected}");
}

#[test]
fn compact_task_progress_counts_states_and_only_suggests_ready_current_work() {
    use kvist::task_queue::{Task, TaskKind, TaskStatus, TaskTimestamps};
    let project = tempfile::tempdir().unwrap();
    kvist::init::initialize(project.path()).unwrap();
    let path = project.path().join("src/TODOS.yaml");
    let mut queue = kvist::task_queue::parse(&fs::read_to_string(&path).unwrap()).unwrap();
    let timestamp: kvist::task_queue::Timestamp =
        serde_json::from_str("\"2026-11-04T08:54:29Z\"").unwrap();
    let template = Task {
        id: "pending-work".into(),
        title: "Test compact progress".into(),
        description: "Expose deterministic progress and next actions.".into(),
        context: "Read-only status fixture.".into(),
        purpose: "Verify all workflow state counts.".into(),
        expected_outcome: "Counts are explicit; blocked components do not suggest execution."
            .into(),
        kind: TaskKind::Test,
        status: TaskStatus::Pending,
        depends_on: vec![],
        requirements: vec!["REQUIREMENTS.md#Task-selection-and-state-updates".into()],
        timestamps: TaskTimestamps {
            created_at: timestamp.clone(),
            updated_at: timestamp,
            completed_at: None,
            recovered: false,
        },
        blocked_reason: None,
        recovery_state: None,
        acceptance_id: None,
        disposition: None,
    };
    for (id, status) in [
        ("completed", TaskStatus::Completed),
        ("active", TaskStatus::InProgress),
        ("blocked", TaskStatus::Blocked),
        ("decision", TaskStatus::AwaitingDecision),
        ("pending-work", TaskStatus::Pending),
    ] {
        let mut task = template.clone();
        task.id = id.into();
        task.status = status;
        if status == TaskStatus::Completed {
            task.timestamps.completed_at = Some(task.timestamps.updated_at.clone());
        }
        if matches!(status, TaskStatus::Blocked | TaskStatus::AwaitingDecision) {
            task.blocked_reason = Some("Explicit human arbitration required.".into());
        }
        queue.tasks.push(task);
    }
    fs::write(&path, kvist::task_queue::serialize(&queue).unwrap()).unwrap();
    let overview = text(run(project.path(), &["status"]));
    assert!(
        overview.contains("1/5 completed; 1 in-progress; 1 blocked; 1 awaiting-decision"),
        "{overview}"
    );
    assert!(!overview.contains("kvist task run"), "{overview}");
    assert!(
        !overview.contains("Explicit human arbitration required."),
        "{overview}"
    );
    let details = text(run(project.path(), &["status", "."]));
    assert!(
        details.contains("Explicit human arbitration required."),
        "{details}"
    );
    assert!(!details.contains("kvist task run"), "{details}");
    queue.tasks = vec![template];
    fs::write(&path, kvist::task_queue::serialize(&queue).unwrap()).unwrap();
    let ready = text(run(project.path(), &["status"]));
    assert!(ready.contains("kvist task run . pending-work"), "{ready}");
    fs::write(project.path().join("VISION.md"), "# Invalid vision\n").unwrap();
    let unavailable = text(run(project.path(), &["status", "."]));
    assert!(!unavailable.contains("kvist task run"), "{unavailable}");
    assert!(
        unavailable.contains("Execution unavailable"),
        "{unavailable}"
    );
}

#[test]
fn explicit_project_and_positional_project_targets_remain_distinct_from_components() {
    let project = project();
    let outside = tempfile::tempdir().unwrap();
    let target = project.path().to_str().unwrap();
    let overview = text(run(outside.path(), &["status", target]));
    assert!(!overview.contains("## Component:"), "{overview}");
    let scoped = text(run(
        outside.path(),
        &["status", "--project", target, "alpha"],
    ));
    assert_eq!(scoped.matches("## Component:").count(), 1, "{scoped}");
    assert!(scoped.contains("## Component: alpha\n"));
    let root = text(run(project.path(), &["status", "."]));
    assert_eq!(root.matches("## Component:").count(), 1, "{root}");
    assert!(root.contains("## Component: .\n"), "{root}");
    let explicit_project = text(run(
        project.path(),
        &["status", "--project", ".", "--details"],
    ));
    assert_eq!(
        explicit_project.matches("## Component:").count(),
        7,
        "{explicit_project}"
    );
    let nested = text(run(&project.path().join("src/alpha"), &["status"]));
    assert!(nested.contains("| beta |"), "{nested}");
}

#[test]
fn unknown_and_escaping_components_fail_instead_of_showing_the_project() {
    let project = project();
    for argument in ["unknown", "alpha/missing", "../alpha"] {
        let output = run(project.path(), &["status", argument]);
        assert!(!output.status.success(), "{argument}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains("not discovered")
                || error.contains("not a discovered component")
                || error.contains("must be `.` or a normal path"),
            "{argument}: {error}"
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("| Component |"));
    }
}

#[test]
fn compact_status_surfaces_queue_errors_global_diagnostics_and_empty_filters() {
    let project = project();
    fs::write(
        project.path().join("src/beta/TODOS.yaml"),
        "not a valid queue",
    )
    .unwrap();
    let before = fs::read(project.path().join("src/beta/TODOS.yaml")).unwrap();
    let overview = text(run(project.path(), &["status"]));
    assert!(overview.contains("unavailable"), "{overview}");
    assert!(overview.contains("invalid TODO queue"), "{overview}");
    assert_eq!(
        fs::read(project.path().join("src/beta/TODOS.yaml")).unwrap(),
        before
    );
    let unfinished = text(run(project.path(), &["status", "--unfinished"]));
    assert!(!unfinished.contains("| alpha |"), "{unfinished}");
    assert!(unfinished.contains("| beta |"), "{unfinished}");
    fs::write(project.path().join("VISION.md"), "# Invalid vision\n").unwrap();
    let selected = text(run(project.path(), &["status", "alpha"]));
    assert!(selected.contains("**State:** invalid"), "{selected}");
    assert!(selected.contains("VISION.md"), "{selected}");
    assert!(selected.contains("line 1 must be"), "{selected}");
    assert!(selected.contains("kvist repair"), "{selected}");
}

#[test]
fn artifact_filters_imply_details_and_do_not_expand_component_scope() {
    let project = project();
    for (flag, included, excluded) in [
        ("--only-documents", "| DESIGN.md |", "| IMPL.md |"),
        ("--only-impls", "| IMPL.md |", "| DESIGN.md |"),
    ] {
        let selected = text(run(project.path(), &["status", "alpha", flag]));
        let component = selected.split("## Component:").nth(1).unwrap();
        assert!(component.contains(included), "{selected}");
        assert!(!component.contains(excluded), "{selected}");
        assert_eq!(selected.matches("## Component:").count(), 1);
        assert_eq!(
            text(run(project.path(), &["status", flag]))
                .matches("## Component:")
                .count(),
            7
        );
    }
    let project = tempfile::tempdir().unwrap();
    kvist::init::initialize(project.path()).unwrap();
    let output = text(run(project.path(), &["status", "--unfinished"]));
    assert!(output.contains("No components match"), "{output}");
}
