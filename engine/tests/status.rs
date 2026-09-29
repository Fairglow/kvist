use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use kvist::init::initialize;
use tempfile::TempDir;

const GENERATED_REQUIREMENTS_REVISION: &str =
    "sha256:bd53663c2dc76fdcbe58b111c0174a7550a3e3fe773a1c3e4a14196c1089dfa0";
const GENERATED_CONTRACT_REVISION: &str =
    "sha256:54b07fd8cbfb911f7e8546854b49944eb499429ea14cf302c2cba0f64238b98c";
const GENERATED_DESIGN_REVISION: &str =
    "sha256:6d6579ce1b018dce3b34e87afd72b494d27692ada8d54003b0a10ecd74abed17";

fn run_kvist(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .output()
        .expect("run kvist command")
}

fn valid_queue(requirements_revision: &str, parent_revision: Option<&str>, tasks: &str) -> String {
    let parent = match parent_revision {
        Some(revision) => {
            format!("  parent_contract:\n    path: ../CONTRACT.md\n    revision: {revision}\n")
        }
        None => "  parent_contract: null\n".to_owned(),
    };
    format!(
        "schema_version: 1\ncomponent:\n  requirements_revision: {requirements_revision}\n  contract_revision: {GENERATED_CONTRACT_REVISION}\n  design_revision: {GENERATED_DESIGN_REVISION}\n{parent}  revalidation:\n    state: current\n    checked_at: 2026-08-16T12:19:23Z\n    stale_since: null\n    causes: []\ntasks:{tasks}\n"
    )
}

fn copy_valid_component_artifacts(project: &TempDir, relative_path: &str) {
    let component = project.path().join("src").join(relative_path);
    fs::create_dir_all(&component).expect("create component");
    fs::copy(
        project.path().join("src/REQUIREMENTS.md"),
        component.join("REQUIREMENTS.md"),
    )
    .expect("copy requirements");
    fs::copy(
        project.path().join("src/CONTRACT.md"),
        component.join("CONTRACT.md"),
    )
    .expect("copy contract");
    fs::copy(
        project.path().join("src/DESIGN.md"),
        component.join("DESIGN.md"),
    )
    .expect("copy design");
    fs::copy(
        project.path().join("src/IMPL.md"),
        component.join("IMPL.md"),
    )
    .expect("copy docs");
}

#[test]
fn status_reports_a_current_initialized_project_in_stable_text_and_json() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    let project_path = project.path().to_string_lossy();
    let project_path_escaped = project_path.replace('\\', "\\\\");

    let text = run_kvist(&[
        "status",
        "--format",
        "text",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(text.status.success());
    assert!(text.stderr.is_empty());
    assert_eq!(
        String::from_utf8(text.stdout).expect("UTF-8 text output"),
        format!(
            "status-format-version: 1\nproject: {project_path_escaped}\nproject-state: current\ncomponent-root: src\ncomponent: . state: current\n  REQUIREMENTS.md: valid\n  CONTRACT.md: valid\n  DESIGN.md: valid\n  TODOS.yaml: valid\n  IMPL.md: valid\n  revalidation-causes: []\n"
        )
    );

    let json = run_kvist(&[
        "status",
        "--format",
        "json",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(json.status.success());
    assert!(json.stderr.is_empty());
    let output = String::from_utf8(json.stdout).expect("UTF-8 JSON output");
    assert!(output.starts_with("{\"format_version\":1,\"project_path\":"));
    assert!(output.contains(
        "\"project_state\":\"current\",\"component_root\":\"src\",\"components\":[{\"path\":\".\",\"state\":\"current\",\"artifacts\":[{\"path\":\"REQUIREMENTS.md\",\"state\":\"valid\"},{\"path\":\"CONTRACT.md\",\"state\":\"valid\"},{\"path\":\"DESIGN.md\",\"state\":\"valid\"},{\"path\":\"TODOS.yaml\",\"state\":\"valid\"},{\"path\":\"IMPL.md\",\"state\":\"valid\"}],\"revalidation_causes\":[]}],\"discovery_error\":null}"
    ));
}

#[test]
fn status_surfaces_component_missing_unsupported_stale_and_blocked_states_without_writes() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    let missing = project.path().join("src/missing");
    fs::create_dir_all(&missing).expect("create incomplete component");
    fs::copy(
        project.path().join("src/REQUIREMENTS.md"),
        missing.join("REQUIREMENTS.md"),
    )
    .expect("copy requirements");

    copy_valid_component_artifacts(&project, "unsupported");
    fs::write(
        project.path().join("src/unsupported/TODOS.yaml"),
        "schema_version: 99\ntasks: []\n",
    )
    .expect("write unsupported queue");

    copy_valid_component_artifacts(&project, "stale");
    fs::write(
        project.path().join("src/stale/TODOS.yaml"),
        valid_queue(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Some(GENERATED_CONTRACT_REVISION),
            " []",
        ),
    )
    .expect("write stale queue");

    copy_valid_component_artifacts(&project, "parent-stale");
    fs::write(
        project.path().join("src/parent-stale/TODOS.yaml"),
        valid_queue(
            GENERATED_REQUIREMENTS_REVISION,
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            " []",
        ),
    )
    .expect("write parent-stale queue");

    fs::write(
        project.path().join("src/TODOS.yaml"),
        valid_queue(
            GENERATED_REQUIREMENTS_REVISION,
            None,
            r#"
  - id: investigate
    title: Investigate status fixture
    description: Preserve an actionable blocked task for status testing.
    context: Status must surface queue blockers without selecting work.
    purpose: Verify that blocked workflow evidence is visible.
    expected_outcome: The component report identifies the blocked queue.
    kind: test
    status: blocked
    depends_on: []
    requirements:
      - REQUIREMENTS.md#Project-status-inspection
    timestamps:
      created_at: 2026-08-16T12:19:23Z
      updated_at: 2026-08-16T12:19:23Z
      completed_at: null
    blocked_reason: Awaiting explicit product decision.
"#,
        ),
    )
    .expect("write blocked root queue");
    let before = fs::read(project.path().join("src/TODOS.yaml")).expect("read queue before");

    let output = run_kvist(&[
        "status",
        "--format",
        "text",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 text output");
    assert!(stdout.contains("component: . state: blocked"));
    assert!(stdout.contains("component: missing state: missing"));
    assert!(stdout.contains("component: stale state: stale"));
    assert!(stdout.contains(
        "cause: component-requirements-revision-changed REQUIREMENTS.md expected sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    ));
    assert!(stdout.contains(
        "component: parent-stale state: stale\n  REQUIREMENTS.md: valid\n  CONTRACT.md: valid\n  DESIGN.md: valid\n  TODOS.yaml: valid\n  IMPL.md: valid\n  cause: parent-contract-revision-changed ../CONTRACT.md expected sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    ));
    assert!(stdout.contains("component: unsupported state: unsupported-version"));
    assert_eq!(
        fs::read(project.path().join("src/TODOS.yaml")).expect("read queue after"),
        before
    );
}

#[test]
#[cfg(unix)]
fn status_escapes_control_characters_in_text_component_paths() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    copy_valid_component_artifacts(&project, "stale\nforged");
    fs::write(
        project.path().join("src/stale\nforged/TODOS.yaml"),
        valid_queue(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Some(GENERATED_CONTRACT_REVISION),
            " []",
        ),
    )
    .expect("write stale queue");

    let output = run_kvist(&[
        "status",
        "--format",
        "text",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 text output");
    assert!(stdout.contains("component: stale\\nforged state: stale"));
    assert!(!stdout.contains("component: stale\nforged state: stale"));
}

#[test]
fn status_filters_components_and_artifacts() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    // Let's create an unfinished (stale) component
    copy_valid_component_artifacts(&project, "stale-comp");
    fs::write(
        project.path().join("src/stale-comp/TODOS.yaml"),
        valid_queue(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Some(GENERATED_CONTRACT_REVISION),
            " []",
        ),
    )
    .expect("write stale queue");

    // 1. Test the stable text status first (shows current root, stale-comp, and all artifacts)
    let default_output = run_kvist(&[
        "status",
        "--format",
        "text",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(default_output.status.success());
    let default_stdout = String::from_utf8(default_output.stdout).expect("UTF-8 text output");
    assert!(default_stdout.contains("component: . state: current"));
    assert!(default_stdout.contains("component: stale-comp state: stale"));
    assert!(default_stdout.contains("  REQUIREMENTS.md: valid"));
    assert!(default_stdout.contains("  CONTRACT.md: valid"));
    assert!(default_stdout.contains("  DESIGN.md: valid"));
    assert!(default_stdout.contains("  TODOS.yaml: valid"));
    assert!(default_stdout.contains("  IMPL.md: valid"));
    assert!(default_stdout.contains("  revalidation-causes: []"));

    // 2. Test --only-documents
    let documents_output = run_kvist(&[
        "status",
        "--format",
        "text",
        "--only-documents",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(documents_output.status.success());
    let documents_stdout = String::from_utf8(documents_output.stdout).expect("UTF-8 text output");
    assert!(documents_stdout.contains("component: . state: current"));
    assert!(documents_stdout.contains("  REQUIREMENTS.md: valid"));
    assert!(documents_stdout.contains("  CONTRACT.md: valid"));
    assert!(documents_stdout.contains("  DESIGN.md: valid"));
    assert!(!documents_stdout.contains("  TODOS.yaml: valid"));
    assert!(!documents_stdout.contains("  IMPL.md: valid"));
    assert!(
        documents_stdout.contains("revalidation-causes: []")
            || documents_stdout.contains("cause: component-requirements-revision-changed")
    );

    // 3. Test --only-impls
    let impls_output = run_kvist(&[
        "status",
        "--format",
        "text",
        "--only-impls",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(impls_output.status.success());
    let impls_stdout = String::from_utf8(impls_output.stdout).expect("UTF-8 text output");
    assert!(impls_stdout.contains("component: . state: current"));
    assert!(!impls_stdout.contains("  REQUIREMENTS.md: valid"));
    assert!(!impls_stdout.contains("  CONTRACT.md: valid"));
    assert!(!impls_stdout.contains("  DESIGN.md: valid"));
    assert!(!impls_stdout.contains("  TODOS.yaml: valid"));
    assert!(impls_stdout.contains("  IMPL.md: valid"));
    // Revalidation causes are excluded from --only-impls
    assert!(!impls_stdout.contains("revalidation-causes"));

    // 4. Test --unfinished (omits current, shows stale-comp)
    let unfinished_output = run_kvist(&[
        "status",
        "--format",
        "text",
        "--unfinished",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(unfinished_output.status.success());
    let unfinished_stdout = String::from_utf8(unfinished_output.stdout).expect("UTF-8 text output");
    assert!(!unfinished_stdout.contains("component: . state: current"));
    assert!(unfinished_stdout.contains("component: stale-comp state: stale"));

    // 5. Test JSON with --unfinished and --only-impls
    let json_output = run_kvist(&[
        "status",
        "--format",
        "json",
        "--unfinished",
        "--only-impls",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(json_output.status.success());
    let json_stdout = String::from_utf8(json_output.stdout).expect("UTF-8 JSON output");
    assert!(!json_stdout.contains("\"path\":\".\""));
    assert!(json_stdout.contains("\"path\":\"stale-comp\""));
    assert!(json_stdout.contains("\"path\":\"IMPL.md\""));
    assert!(!json_stdout.contains("\"path\":\"REQUIREMENTS.md\""));
    assert!(!json_stdout.contains("\"path\":\"CONTRACT.md\""));
    assert!(!json_stdout.contains("\"path\":\"DESIGN.md\""));
    assert!(!json_stdout.contains("\"path\":\"TODOS.yaml\""));
}

#[test]
fn status_transparent_namespace_parent_contract() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    // Let's create a transparent layout: ordinary/component
    let component_dir = project.path().join("src/ordinary/component");
    fs::create_dir_all(&component_dir).expect("create component");
    fs::copy(
        project.path().join("src/REQUIREMENTS.md"),
        component_dir.join("REQUIREMENTS.md"),
    )
    .expect("copy requirements");
    fs::copy(
        project.path().join("src/CONTRACT.md"),
        component_dir.join("CONTRACT.md"),
    )
    .expect("copy contract");
    fs::copy(
        project.path().join("src/DESIGN.md"),
        component_dir.join("DESIGN.md"),
    )
    .expect("copy design");
    fs::copy(
        project.path().join("src/IMPL.md"),
        component_dir.join("IMPL.md"),
    )
    .expect("copy docs");

    // The child records the root contract through a transparent directory.
    let child_queue = valid_queue(
        GENERATED_REQUIREMENTS_REVISION,
        Some(GENERATED_CONTRACT_REVISION),
        " []",
    )
    .replace("path: ../CONTRACT.md", "path: ../../CONTRACT.md");
    fs::write(component_dir.join("TODOS.yaml"), child_queue).expect("write queue");

    // Let's run status
    let output = run_kvist(&[
        "status",
        "--format",
        "text",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 text output");
    // Ensure both . and ordinary/component are discovered and current
    assert!(stdout.contains("component: . state: current"));
    let ordinary_component = Path::new("ordinary")
        .join("component")
        .to_string_lossy()
        .into_owned()
        .replace('\\', "\\\\");
    assert!(stdout.contains(&format!("component: {ordinary_component} state: current")));
    assert!(!stdout.contains("cause:"));

    // Modify the root contract to trigger parent-contract staleness.
    let root_contract_path = project.path().join("src/CONTRACT.md");
    let mut contents = fs::read_to_string(&root_contract_path).expect("read root contract");
    contents.push_str("\n\n<!-- dynamic change -->\n");
    fs::write(&root_contract_path, contents).expect("modify root contract");

    let output_stale = run_kvist(&[
        "status",
        "--format",
        "text",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output_stale.status.success());
    let stdout_stale = String::from_utf8(output_stale.stdout).expect("UTF-8 text output");
    // The diagnostic path crosses the transparent directory.
    assert!(stdout_stale.contains(&format!("component: {ordinary_component} state: stale")));
    let relative_contract = "../../CONTRACT.md";
    assert!(stdout_stale.contains(&format!(
        "cause: parent-contract-revision-changed {relative_contract}"
    )));
}

#[test]
fn status_attributes_contract_and_design_changes_independently() {
    for (filename, cause) in [
        ("CONTRACT.md", "component-contract-revision-changed"),
        ("DESIGN.md", "component-design-revision-changed"),
    ] {
        let project = TempDir::new().expect("project");
        initialize(project.path()).expect("initialize");
        let path = project.path().join("src").join(filename);
        let mut contents = fs::read_to_string(&path).expect("read component document");
        contents.push_str("\n\n<!-- changed -->\n");
        fs::write(path, contents).expect("change component document");

        let output = run_kvist(&[
            "status",
            "--format",
            "text",
            project.path().to_str().expect("UTF-8 project path"),
        ]);
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 status");
        assert!(stdout.contains(&format!("cause: {cause} {filename}")));
    }
}

#[test]
fn parent_requirements_and_design_do_not_stale_a_child() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    copy_valid_component_artifacts(&project, "child");
    fs::write(
        project.path().join("src/child/TODOS.yaml"),
        valid_queue(
            GENERATED_REQUIREMENTS_REVISION,
            Some(GENERATED_CONTRACT_REVISION),
            " []",
        ),
    )
    .expect("write child queue");

    for filename in ["REQUIREMENTS.md", "DESIGN.md"] {
        let path = project.path().join("src").join(filename);
        let mut contents = fs::read_to_string(&path).expect("read parent document");
        contents.push_str("\n\n<!-- parent-internal change -->\n");
        fs::write(path, contents).expect("change parent document");
    }

    let output = run_kvist(&[
        "status",
        "--format",
        "text",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 status");
    assert!(stdout.contains("component: . state: stale"));
    assert!(stdout.contains("component: child state: current"));
}

#[test]
fn status_overview_reports_compact_docs_and_task_progress() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    // Test with --format overview
    let output = run_kvist(&[
        "status",
        "--format",
        "overview",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 status");
    assert!(stdout.contains("Project Status"));
    assert!(stdout.contains("Documents: all valid (5/5)"));
    assert!(stdout.contains("Progress:"));

    // A bare `status` defaults to the same human-friendly view.
    let output2 = run_kvist(&[
        "status",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8(output2.stdout).expect("UTF-8 status");
    assert!(stdout2.contains("Project Status"));
    assert!(stdout2.contains("Documents: all valid (5/5)"));
}

#[test]
fn status_overview_lists_changed_documents_with_revisions_and_diff_hint() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    let design = project.path().join("src").join("DESIGN.md");
    let mut contents = fs::read_to_string(&design).expect("read design");
    contents.push_str("\n\n<!-- changed -->\n");
    fs::write(design, contents).expect("change design");

    let output = run_kvist(&[
        "status",
        "--format",
        "overview",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 overview");
    assert!(stdout.contains("Component: ."));
    assert!(stdout.contains("State:     stale"));
    assert!(stdout.contains("Changed:   DESIGN.md"));
    assert!(stdout.contains("expected  sha256:"));
    assert!(stdout.contains("observed  sha256:"));
    assert!(stdout.contains("git diff HEAD -- src/DESIGN.md"));
    assert!(stdout.contains("kvist component accept ."));
}

/// A blocked component lists each blocked task with its reason and the exact
/// next command, plus any awaiting-decision tasks, in both text and overview.
#[test]
fn status_blocked_details_list_reasons_and_exact_next_commands() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    fs::write(
        project.path().join("src/TODOS.yaml"),
        valid_queue(
            GENERATED_REQUIREMENTS_REVISION,
            None,
            r#"
  - id: dep-task
    title: Dependency task
    description: A pending dependency for a blocked task.
    context: Fixture for blocked guidance.
    purpose: Keep one dependency incomplete.
    expected_outcome: A blocked task with an incomplete dependency chain.
    kind: test
    status: pending
    depends_on: []
    requirements:
      - REQUIREMENTS.md#Project-status-inspection
    timestamps:
      created_at: 2026-08-16T12:19:23Z
      updated_at: 2026-08-16T12:19:23Z
      completed_at: null
    blocked_reason: null
    recovery_state: null
  - id: blocked-ready
    title: Blocked ready task
    description: A blocked task whose dependency chain is completed.
    context: Fixture for blocked guidance.
    purpose: Verify the run next-command.
    expected_outcome: The report offers task run for this task.
    kind: test
    status: blocked
    depends_on: []
    requirements:
      - REQUIREMENTS.md#Project-status-inspection
    timestamps:
      created_at: 2026-08-16T12:19:23Z
      updated_at: 2026-08-16T12:19:23Z
      completed_at: null
    blocked_reason: missing test-command policy for component `x`. Please define a command for it in `kvist.toml` under `[test_policy]` and approve it
    recovery_state: null
  - id: blocked-waiting
    title: Blocked waiting task
    description: A blocked task whose dependency chain is not completed.
    context: Fixture for blocked guidance.
    purpose: Verify the transition next-command.
    expected_outcome: The report offers task transition pending for this task.
    kind: test
    status: blocked
    depends_on:
      - dep-task
    requirements:
      - REQUIREMENTS.md#Project-status-inspection
    timestamps:
      created_at: 2026-08-16T12:19:23Z
      updated_at: 2026-08-16T12:19:23Z
      completed_at: null
    blocked_reason: Waiting on dep-task.
    recovery_state: null
  - id: paused
    title: Paused task
    description: An awaiting-decision task.
    context: Fixture for blocked guidance.
    purpose: Verify decision visibility.
    expected_outcome: The report lists the decision with a help pointer.
    kind: test
    status: awaiting-decision
    depends_on: []
    requirements:
      - REQUIREMENTS.md#Project-status-inspection
    timestamps:
      created_at: 2026-08-16T12:19:23Z
      updated_at: 2026-08-16T12:19:23Z
      completed_at: null
    blocked_reason: Awaiting decision on product direction.
    recovery_state: null
"#,
        ),
    )
    .expect("write fixture queue");

    let text = run_kvist(&[
        "status",
        "--format",
        "text",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(text.status.success());
    let stdout = String::from_utf8(text.stdout).expect("UTF-8 text output");
    assert!(stdout.contains("component: . state: blocked"));
    assert!(stdout.contains(
        "Next Step: 2 blocked task(s) need resolution; 'kvist help task-states' explains states and transitions."
    ));
    assert!(stdout.contains("blocked: blocked-ready"));
    assert!(stdout.contains(
        "reason: missing test-command policy for component `x`. Please define a command for it in `kvist.toml`\u{2026}"
    ));
    assert!(stdout.contains("next: kvist task run . blocked-ready"));
    assert!(stdout.contains("blocked: blocked-waiting"));
    assert!(stdout.contains("reason: Waiting on dep-task."));
    assert!(stdout.contains(
        "next: kvist task transition . blocked-waiting pending (1 dependency task incomplete)"
    ));
    assert!(stdout.contains("decision: paused"));
    assert!(stdout.contains("reason: Awaiting decision on product direction."));
    assert!(
        stdout.contains("next: human decision needed; 'kvist help task-states' explains resuming")
    );

    let overview = run_kvist(&[
        "status",
        "--format",
        "overview",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(overview.status.success());
    let stdout = String::from_utf8(overview.stdout).expect("UTF-8 overview output");
    assert!(stdout.contains("1 awaiting-decision"));
    assert!(stdout.contains(
        "Action:    Resolve the blocked tasks below; 'kvist help task-states' explains each state and its resolution."
    ));
    assert!(stdout.contains("Blocked:   blocked-ready"));
    assert!(stdout.contains(
        "reason: missing test-command policy for component `x`. Please define a command for it in `kvist.toml`\u{2026}"
    ));
    assert!(stdout.contains("next:   kvist task run . blocked-ready"));
    assert!(stdout.contains("Blocked:   blocked-waiting"));
    assert!(stdout.contains(
        "next:   kvist task transition . blocked-waiting pending (1 dependency task incomplete)"
    ));
    assert!(stdout.contains("Decisions: paused"));
    assert!(
        stdout
            .contains("next:   human decision needed; 'kvist help task-states' explains resuming")
    );
}

/// The stable text report names a next step for unsupported-version components.
#[test]
fn status_text_reports_next_step_for_unsupported_version() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    copy_valid_component_artifacts(&project, "unsupported");
    fs::write(
        project.path().join("src/unsupported/TODOS.yaml"),
        "schema_version: 99\ntasks: []\n",
    )
    .expect("write unsupported queue");

    let text = run_kvist(&[
        "status",
        "--format",
        "text",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(text.status.success());
    let stdout = String::from_utf8(text.stdout).expect("UTF-8 text output");
    assert!(stdout.contains("component: unsupported state: unsupported-version"));
    assert!(stdout.contains("Next Step: "));
    assert!(stdout.contains("'kvist help concepts'"));
}
