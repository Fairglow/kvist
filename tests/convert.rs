use std::{
    fs,
    process::{Command, Output},
};

use kvist::{
    component_documents::{self, DocumentKind},
    convert::{ConvertOutcome, convert},
    init::{InitOutcome, initialize},
    task_queue,
};
use tempfile::TempDir;

fn existing_project() -> TempDir {
    let project = TempDir::new().expect("create project");
    fs::create_dir_all(project.path().join("src")).expect("create source directory");
    fs::create_dir_all(project.path().join("tests")).expect("create test directory");
    fs::create_dir_all(project.path().join("benches")).expect("create benchmark directory");
    fs::write(
        project.path().join("Cargo.toml"),
        r#"[package]
name = "converted-example"
version = "1.2.3"
authors = ["Ada <ada@example.invalid>"]
description = "An existing project."

[dependencies]
serde = "1"

[features]
fast = []
"#,
    )
    .expect("write manifest");
    fs::write(project.path().join("src/lib.rs"), "pub fn existing() {}\n").expect("write source");
    fs::write(project.path().join("tests/existing.rs"), "existing test\n").expect("write test");
    fs::write(
        project.path().join("benches/existing.rs"),
        "existing benchmark\n",
    )
    .expect("write benchmark");
    project
}

fn run_init(path: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .arg("init")
        .arg(path)
        .output()
        .expect("run kvist init")
}

#[test]
fn init_converts_an_existing_rust_project_without_changing_implementation_files() {
    let project = existing_project();
    let manifest = fs::read(project.path().join("Cargo.toml")).expect("read manifest");
    let source = fs::read(project.path().join("src/lib.rs")).expect("read source");
    let tests = fs::read(project.path().join("tests/existing.rs")).expect("read tests");
    let benches = fs::read(project.path().join("benches/existing.rs")).expect("read benchmarks");

    let outcome = initialize(project.path()).expect("convert existing project");

    assert_eq!(
        outcome,
        InitOutcome::ConvertedExistingRustProject {
            project_dir: project.path().to_path_buf()
        }
    );
    assert_eq!(
        fs::read(project.path().join("Cargo.toml")).expect("read manifest"),
        manifest
    );
    assert_eq!(
        fs::read(project.path().join("src/lib.rs")).expect("read source"),
        source
    );
    assert_eq!(
        fs::read(project.path().join("tests/existing.rs")).expect("read tests"),
        tests
    );
    assert_eq!(
        fs::read(project.path().join("benches/existing.rs")).expect("read benches"),
        benches
    );

    let metadata = project.path().join(".kvist");
    for kind in [
        DocumentKind::Requirements,
        DocumentKind::Contract,
        DocumentKind::Design,
    ] {
        assert!(
            component_documents::validate_file(kind, &metadata.join(kind.filename()))
                .expect("validate generated component document")
                .is_valid()
        );
    }
    let queue = fs::read_to_string(metadata.join("TODOS.yaml")).expect("read generated queue");
    task_queue::parse(&queue).expect("validate generated queue");
    assert!(queue.contains("converted-example 1.2.3"));
    assert!(queue.contains("serde"));
    assert!(queue.contains("fast"));
    assert!(metadata.join("IMPL.md").is_file());
    assert!(metadata.join("COMPLIANCE_REVIEW.md").is_file());
}

#[test]
fn conversion_is_no_clobber_when_metadata_already_exists() {
    let project = existing_project();
    convert(project.path()).expect("initial conversion");
    let requirements_path = project.path().join(".kvist/REQUIREMENTS.md");
    fs::write(&requirements_path, "human-edited requirements\n").expect("edit requirements");

    let outcome = convert(project.path()).expect("repeated conversion");

    assert_eq!(
        outcome,
        ConvertOutcome::AlreadyConverted {
            project_dir: project.path().to_path_buf()
        }
    );
    assert_eq!(
        fs::read_to_string(requirements_path).expect("read requirements"),
        "human-edited requirements\n"
    );
}

#[test]
fn cli_init_reports_existing_project_conversion() {
    let project = existing_project();

    let output = run_init(project.path());

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("draft Kvist artifacts"));
    assert!(output.stderr.is_empty());
}

#[test]
fn init_keeps_normal_initialization_when_a_manifest_has_no_source_directory() {
    let project = TempDir::new().expect("create project");
    fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname = \"incomplete\"\nversion = \"0.1.0\"\n",
    )
    .expect("write manifest");

    let outcome = initialize(project.path()).expect("normally initialize project");

    assert!(matches!(outcome, InitOutcome::Initialized { .. }));
    assert!(project.path().join("src/REQUIREMENTS.md").is_file());
    assert!(project.path().join("src/CONTRACT.md").is_file());
    assert!(project.path().join("src/DESIGN.md").is_file());
    assert!(!project.path().join(".kvist").exists());
}

#[test]
fn conversion_refuses_an_unrelated_metadata_directory() {
    let project = existing_project();
    fs::create_dir(project.path().join(".kvist")).expect("create unrelated metadata");

    let error = convert(project.path()).expect_err("metadata conflict must be explicit");

    assert!(error.to_string().contains("Kvist artifacts already exist"));
    assert!(!project.path().join(".kvist/REQUIREMENTS.md").exists());
}
