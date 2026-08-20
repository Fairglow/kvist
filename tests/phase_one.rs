use std::{
    path::Path,
    process::{Command, Output},
};

use tempfile::TempDir;

fn run_kvist(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .output()
        .expect("run kvist command")
}

#[test]
fn phase_one_cli_workflow_initializes_generates_validates_and_renders() {
    let project = TempDir::new().expect("create temporary project");
    let component = project.path().join("src/network");
    let component_path = component.to_str().expect("UTF-8 component path");
    let project_path = project.path().to_str().expect("UTF-8 project path");
    let specification = component.join("SPEC.md");
    let specification_path = specification.to_str().expect("UTF-8 specification path");

    let init = run_kvist(&["init", project_path]);
    assert!(init.status.success());
    assert!(init.stderr.is_empty());

    let generate = run_kvist(&["spec", "new", component_path]);
    assert!(generate.status.success());
    assert!(generate.stderr.is_empty());

    let validate = run_kvist(&["spec", "validate", specification_path]);
    assert!(validate.status.success());
    assert!(validate.stderr.is_empty());

    let tree = run_kvist(&["tree", project_path]);
    assert!(tree.status.success());
    assert!(tree.stderr.is_empty());
    assert!(
        String::from_utf8_lossy(&tree.stdout)
            .contains("network [incomplete: missing TODOS.yaml; missing IMPL.md]")
    );

    assert!(Path::new(specification_path).is_file());
}
