use std::{
    fs,
    process::{Command, Output},
};

use kvist::component_documents::{
    COMPONENT_CONTRACT_TEMPLATE, COMPONENT_DESIGN_TEMPLATE, COMPONENT_REQUIREMENTS_TEMPLATE,
    DocumentKind, validate_file,
};
use tempfile::TempDir;

fn run_component(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .output()
        .expect("run kvist component")
}

#[test]
fn component_new_creates_all_deterministic_valid_documents() {
    let workspace = TempDir::new().expect("create temporary workspace");
    let component = workspace.path().join("src/network");

    let output = run_component(&["component", "new", component.to_str().expect("UTF-8 path")]);

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("created component documents"));
    assert!(output.stderr.is_empty());
    for (kind, template) in [
        (DocumentKind::Requirements, COMPONENT_REQUIREMENTS_TEMPLATE),
        (DocumentKind::Contract, COMPONENT_CONTRACT_TEMPLATE),
        (DocumentKind::Design, COMPONENT_DESIGN_TEMPLATE),
    ] {
        let path = component.join(kind.filename());
        assert_eq!(fs::read_to_string(&path).expect("read document"), template);
        assert!(
            validate_file(kind, &path)
                .expect("validate document")
                .is_valid()
        );
    }
}

#[test]
fn component_new_refuses_the_complete_set_when_any_document_exists() {
    let component = TempDir::new().expect("create temporary component");
    let contract = component.path().join("CONTRACT.md");
    fs::write(&contract, "user-authored contract").expect("create contract");

    let output = run_component(&[
        "component",
        "new",
        component.path().to_str().expect("UTF-8 component path"),
    ]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));
    assert!(!component.path().join("REQUIREMENTS.md").exists());
    assert!(!component.path().join("DESIGN.md").exists());
    assert_eq!(
        fs::read_to_string(contract).expect("read existing contract"),
        "user-authored contract"
    );
}

#[test]
fn component_validate_reports_the_document_with_line_aware_errors() {
    let component = TempDir::new().expect("create temporary component");
    fs::write(
        component.path().join("REQUIREMENTS.md"),
        COMPONENT_REQUIREMENTS_TEMPLATE,
    )
    .expect("write requirements");
    fs::write(
        component.path().join("CONTRACT.md"),
        COMPONENT_CONTRACT_TEMPLATE,
    )
    .expect("write contract");
    fs::write(component.path().join("DESIGN.md"), "# invalid\n").expect("write design");

    let output = run_component(&[
        "component",
        "validate",
        component.path().to_str().expect("UTF-8 component path"),
    ]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("DESIGN.md"));
    assert!(stderr.contains("1:1: expected the document version marker"));
}
