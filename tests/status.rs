use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use kvist::init::initialize;
use tempfile::TempDir;

const GENERATED_SPECIFICATION_REVISION: &str =
    "sha256:d47faba18fc80961e3cf1872cbd0d74ccc114a9667dfbc6b84dbbfac2234a1bd";

fn run_kvist(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .output()
        .expect("run kvist command")
}

fn valid_queue(specification_revision: &str, parent_revision: Option<&str>, tasks: &str) -> String {
    let parent = match parent_revision {
        Some(revision) => {
            format!("  parent_specification:\n    path: ../SPEC.md\n    revision: {revision}\n")
        }
        None => "  parent_specification: null\n".to_owned(),
    };
    format!(
        "schema_version: 1\ncomponent:\n  specification_revision: {specification_revision}\n{parent}  revalidation:\n    state: current\n    checked_at: 2026-08-16T12:19:23Z\n    stale_since: null\n    causes: []\ntasks:{tasks}\n"
    )
}

fn copy_valid_component_artifacts(project: &TempDir, relative_path: &str) {
    let component = project.path().join("src").join(relative_path);
    fs::create_dir_all(&component).expect("create component");
    fs::copy(
        project.path().join("src/SPEC.md"),
        component.join("SPEC.md"),
    )
    .expect("copy spec");
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
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(text.status.success());
    assert!(text.stderr.is_empty());
    assert_eq!(
        String::from_utf8(text.stdout).expect("UTF-8 text output"),
        format!(
            "status-format-version: 1\nproject: {project_path_escaped}\nproject-state: current\ncomponent-root: src\ncomponent: . state: current\n  SPEC.md: valid\n  TODOS.yaml: valid\n  IMPL.md: valid\n  revalidation-causes: []\n"
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
        "\"project_state\":\"current\",\"component_root\":\"src\",\"components\":[{\"path\":\".\",\"state\":\"current\",\"artifacts\":[{\"path\":\"SPEC.md\",\"state\":\"valid\"},{\"path\":\"TODOS.yaml\",\"state\":\"valid\"},{\"path\":\"IMPL.md\",\"state\":\"valid\"}],\"revalidation_causes\":[]}],\"discovery_error\":null}"
    ));
}

#[test]
fn status_surfaces_component_missing_unsupported_stale_and_blocked_states_without_writes() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    let missing = project.path().join("src/missing");
    fs::create_dir_all(&missing).expect("create incomplete component");
    fs::copy(project.path().join("src/SPEC.md"), missing.join("SPEC.md")).expect("copy spec");

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
            Some(GENERATED_SPECIFICATION_REVISION),
            " []",
        ),
    )
    .expect("write stale queue");

    copy_valid_component_artifacts(&project, "parent-stale");
    fs::write(
        project.path().join("src/parent-stale/TODOS.yaml"),
        valid_queue(
            GENERATED_SPECIFICATION_REVISION,
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            " []",
        ),
    )
    .expect("write parent-stale queue");

    fs::write(
        project.path().join("src/TODOS.yaml"),
        valid_queue(
            GENERATED_SPECIFICATION_REVISION,
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
      - SPEC.md#Project-status-inspection
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
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 text output");
    assert!(stdout.contains("component: . state: blocked"));
    assert!(stdout.contains("component: missing state: missing"));
    assert!(stdout.contains("component: stale state: stale"));
    assert!(stdout.contains(
        "cause: component-specification-revision-changed SPEC.md expected sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    ));
    assert!(stdout.contains(
        "component: parent-stale state: stale\n  SPEC.md: valid\n  TODOS.yaml: valid\n  IMPL.md: valid\n  cause: parent-specification-revision-changed ../SPEC.md expected sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
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
            Some(GENERATED_SPECIFICATION_REVISION),
            " []",
        ),
    )
    .expect("write stale queue");

    let output = run_kvist(&[
        "status",
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
            Some(GENERATED_SPECIFICATION_REVISION),
            " []",
        ),
    )
    .expect("write stale queue");

    // 1. Test default status output first (shows current root, stale-comp, and all artifacts)
    let default_output = run_kvist(&[
        "status",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(default_output.status.success());
    let default_stdout = String::from_utf8(default_output.stdout).expect("UTF-8 text output");
    assert!(default_stdout.contains("component: . state: current"));
    assert!(default_stdout.contains("component: stale-comp state: stale"));
    assert!(default_stdout.contains("  SPEC.md: valid"));
    assert!(default_stdout.contains("  TODOS.yaml: valid"));
    assert!(default_stdout.contains("  IMPL.md: valid"));
    assert!(default_stdout.contains("  revalidation-causes: []"));

    // 2. Test --only-specs
    let specs_output = run_kvist(&[
        "status",
        "--only-specs",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(specs_output.status.success());
    let specs_stdout = String::from_utf8(specs_output.stdout).expect("UTF-8 text output");
    assert!(specs_stdout.contains("component: . state: current"));
    assert!(specs_stdout.contains("  SPEC.md: valid"));
    assert!(!specs_stdout.contains("  TODOS.yaml: valid"));
    assert!(!specs_stdout.contains("  IMPL.md: valid"));
    // Revalidation causes are spec-centric, so they should be present
    assert!(
        specs_stdout.contains("revalidation-causes: []")
            || specs_stdout.contains("cause: component-specification-revision-changed")
    );

    // 3. Test --only-impls
    let impls_output = run_kvist(&[
        "status",
        "--only-impls",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(impls_output.status.success());
    let impls_stdout = String::from_utf8(impls_output.stdout).expect("UTF-8 text output");
    assert!(impls_stdout.contains("component: . state: current"));
    assert!(!impls_stdout.contains("  SPEC.md: valid"));
    assert!(!impls_stdout.contains("  TODOS.yaml: valid"));
    assert!(impls_stdout.contains("  IMPL.md: valid"));
    // Revalidation causes are excluded from --only-impls
    assert!(!impls_stdout.contains("revalidation-causes"));

    // 4. Test --unfinished (omits current, shows stale-comp)
    let unfinished_output = run_kvist(&[
        "status",
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
    assert!(!json_stdout.contains("\"path\":\"SPEC.md\""));
    assert!(!json_stdout.contains("\"path\":\"TODOS.yaml\""));
}

#[test]
fn status_transparent_namespace_parent_specification() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    // Let's create a transparent layout: ordinary/component
    let component_dir = project.path().join("src/ordinary/component");
    fs::create_dir_all(&component_dir).expect("create component");
    fs::copy(
        project.path().join("src/SPEC.md"),
        component_dir.join("SPEC.md"),
    )
    .expect("copy spec");
    fs::copy(
        project.path().join("src/IMPL.md"),
        component_dir.join("IMPL.md"),
    )
    .expect("copy docs");

    // Write a child queue that expects the parent specification at the root (since ordinary is transparent)
    fs::write(
        component_dir.join("TODOS.yaml"),
        valid_queue(
            GENERATED_SPECIFICATION_REVISION,
            Some(GENERATED_SPECIFICATION_REVISION),
            " []",
        ),
    )
    .expect("write queue");

    // Let's run status
    let output = run_kvist(&[
        "status",
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

    // Let's modify the root SPEC.md by appending a line to trigger a parent-specification-revision-changed cause
    let root_spec_path = project.path().join("src/SPEC.md");
    let mut contents = fs::read_to_string(&root_spec_path).expect("read root spec");
    contents.push_str("\n\n<!-- dynamic change -->\n");
    fs::write(&root_spec_path, contents).expect("modify root spec");

    let output_stale = run_kvist(&[
        "status",
        project.path().to_str().expect("UTF-8 project path"),
    ]);
    assert!(output_stale.status.success());
    let stdout_stale = String::from_utf8(output_stale.stdout).expect("UTF-8 text output");
    // Under transparent directories, the relative path from ordinary/component to root SPEC.md is "../../SPEC.md"
    assert!(stdout_stale.contains(&format!("component: {ordinary_component} state: stale")));
    let relative_spec = "../../SPEC.md";
    assert!(stdout_stale.contains(&format!(
        "cause: parent-specification-revision-changed {relative_spec}"
    )));
}
