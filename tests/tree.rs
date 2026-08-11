use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use kvist::{discovery::ComponentArtifact, init::initialize, tree::render_project};
use tempfile::TempDir;

fn create_component(path: &Path, artifacts: &[ComponentArtifact]) {
    fs::create_dir_all(path).expect("create component directory");
    for artifact in artifacts {
        fs::write(path.join(artifact.filename()), "fixture").expect("create component artifact");
    }
}

fn run_tree(path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .arg("tree")
        .arg(path)
        .output()
        .expect("run kvist tree")
}

#[test]
fn cli_renders_a_stable_ascii_tree_with_component_statuses() {
    let project = TempDir::new().expect("create temporary project");
    initialize(project.path()).expect("initialize project");
    let component_root = project.path().join("src");
    create_component(
        &component_root.join("zebra"),
        &[
            ComponentArtifact::Specification,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::Documentation,
        ],
    );
    create_component(
        &component_root.join("alpha"),
        &[ComponentArtifact::Specification],
    );

    let output = run_tree(project.path());

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("tree output is UTF-8"),
        concat!(
            "component root: src\n",
            ". [complete]\n",
            "  alpha [incomplete: missing TODOS.yaml; missing DOCS.md]\n",
            "  zebra [complete]\n",
        )
    );
}

#[test]
fn tree_reports_invalid_component_artifacts() {
    let project = TempDir::new().expect("create temporary project");
    initialize(project.path()).expect("initialize project");
    let broken = project.path().join("src/broken");
    create_component(&broken, &[ComponentArtifact::Specification]);
    fs::create_dir(broken.join("TODOS.yaml")).expect("create invalid task queue");

    let output = render_project(project.path()).expect("render tree");

    assert!(output.contains("broken [invalid: TODOS.yaml is a directory; missing DOCS.md]"));
}

#[test]
fn tree_uses_the_configured_component_root() {
    let project = TempDir::new().expect("create temporary project");
    initialize(project.path()).expect("initialize project");
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"components\"\n[llm]\nprovider = \"none\"\n",
    )
    .expect("configure component root");
    create_component(
        &project.path().join("components"),
        &[
            ComponentArtifact::Specification,
            ComponentArtifact::TaskQueue,
            ComponentArtifact::Documentation,
        ],
    );

    let output = render_project(project.path()).expect("render configured tree");

    assert_eq!(output, "component root: components\n. [complete]");
}

#[test]
fn tree_rejects_a_non_project_directory_with_an_actionable_error() {
    let directory = TempDir::new().expect("create temporary directory");

    let output = run_tree(directory.path());

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("run `kvist init` first"));
}

#[test]
fn tree_rejects_component_roots_that_escape_the_project() {
    let project = TempDir::new().expect("create temporary project");
    initialize(project.path()).expect("initialize project");
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"../outside\"\n",
    )
    .expect("write invalid configuration");

    let error = render_project(project.path()).expect_err("reject escaping root");

    assert!(error.to_string().contains("only normal path segments"));
}
