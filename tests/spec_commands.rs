use std::{
    fs,
    process::{Command, Output},
};

use kvist::specification::{COMPONENT_SPEC_TEMPLATE, validate_file};
use tempfile::TempDir;

fn run_spec(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .output()
        .expect("run kvist spec")
}

#[test]
fn spec_new_creates_a_deterministic_valid_specification_in_a_missing_directory() {
    let workspace = TempDir::new().expect("create temporary workspace");
    let component = workspace.path().join("src/network");

    let output = run_spec(&["spec", "new", component.to_str().expect("UTF-8 path")]);

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("created specification at"));
    assert!(output.stderr.is_empty());
    let specification_path = component.join("SPEC.md");
    assert_eq!(
        fs::read_to_string(&specification_path).expect("read generated specification"),
        COMPONENT_SPEC_TEMPLATE
    );
    assert!(
        validate_file(&specification_path)
            .expect("validate generated specification")
            .is_valid()
    );
}

#[test]
fn spec_new_refuses_to_overwrite_an_existing_specification() {
    let component = TempDir::new().expect("create temporary component");
    let specification_path = component.path().join("SPEC.md");
    fs::write(&specification_path, "user-authored specification").expect("create specification");

    let output = run_spec(&[
        "spec",
        "new",
        component.path().to_str().expect("UTF-8 component path"),
    ]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));
    assert_eq!(
        fs::read_to_string(specification_path).expect("read existing specification"),
        "user-authored specification"
    );
}

#[test]
fn spec_new_rejects_a_file_as_the_component_directory() {
    let workspace = TempDir::new().expect("create temporary workspace");
    let component = workspace.path().join("component");
    fs::write(&component, "not a directory").expect("create component file");

    let output = run_spec(&["spec", "new", component.to_str().expect("UTF-8 path")]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("must be a directory"));
}

#[test]
fn spec_validate_reports_success_for_a_generated_specification() {
    let component = TempDir::new().expect("create temporary component");
    let specification_path = component.path().join("SPEC.md");
    fs::write(&specification_path, COMPONENT_SPEC_TEMPLATE).expect("create specification");

    let output = run_spec(&[
        "spec",
        "validate",
        specification_path
            .to_str()
            .expect("UTF-8 specification path"),
    ]);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        format!("valid specification: {}\n", specification_path.display())
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn spec_validate_reports_line_aware_errors() {
    let component = TempDir::new().expect("create temporary component");
    let specification_path = component.path().join("SPEC.md");
    fs::write(&specification_path, "# invalid\n").expect("create invalid specification");

    let output = run_spec(&[
        "spec",
        "validate",
        specification_path
            .to_str()
            .expect("UTF-8 specification path"),
    ]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("1:1: expected template version marker on line 1")
    );
}
