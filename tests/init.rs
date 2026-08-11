use std::{
    fs,
    process::{Command, Output},
};

use kvist::{
    artifacts::root_artifacts,
    init::{InitOutcome, initialize},
};
use tempfile::TempDir;

fn run_init(path: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .arg("init")
        .arg(path)
        .output()
        .expect("run kvist init")
}

#[test]
fn cli_initializes_an_empty_directory_with_the_complete_artifact_set() {
    let project = TempDir::new().expect("create temporary project");

    let output = run_init(project.path());

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("initialized Kvist project"));
    assert!(output.stderr.is_empty());
    for artifact in root_artifacts() {
        assert_eq!(
            fs::read_to_string(project.path().join(artifact.relative_path))
                .expect("read generated artifact"),
            artifact.contents
        );
    }
}

#[test]
fn initialization_creates_a_missing_nested_project_directory() {
    let workspace = TempDir::new().expect("create temporary workspace");
    let project_dir = workspace.path().join("nested/project");

    let outcome = initialize(&project_dir).expect("initialize nested project");

    assert_eq!(
        outcome,
        InitOutcome::Initialized {
            project_dir: project_dir.clone()
        }
    );
    assert!(project_dir.join("src/SPEC.md").is_file());
}

#[test]
fn initialization_of_a_complete_project_is_idempotent() {
    let project = TempDir::new().expect("create temporary project");
    initialize(project.path()).expect("first initialization");
    let config_path = project.path().join("kvist.toml");
    fs::write(&config_path, "# user change\nschema_version = 1\n")
        .expect("modify existing configuration");

    let outcome = initialize(project.path()).expect("second initialization");

    assert_eq!(
        outcome,
        InitOutcome::AlreadyInitialized {
            project_dir: project.path().to_path_buf()
        }
    );
    assert_eq!(
        fs::read_to_string(config_path).expect("read existing configuration"),
        "# user change\nschema_version = 1\n"
    );
}

#[test]
fn partial_kvist_artifacts_are_rejected_without_overwriting_them() {
    let project = TempDir::new().expect("create temporary project");
    let config = project.path().join("kvist.toml");
    fs::write(&config, "user-authored configuration").expect("create conflicting file");

    let error = initialize(project.path()).expect_err("partial artifacts must be rejected");

    assert!(error.to_string().contains("Kvist artifacts already exist"));
    assert_eq!(
        fs::read_to_string(&config).expect("read conflicting file"),
        "user-authored configuration"
    );
    assert!(!project.path().join("ROOT_CONTRACT.md").exists());
}

#[test]
fn invalid_artifact_parent_prevents_any_artifact_write() {
    let project = TempDir::new().expect("create temporary project");
    fs::write(project.path().join("src"), "not a directory").expect("create conflicting parent");

    let error = initialize(project.path()).expect_err("invalid parent must be rejected");

    assert!(error.to_string().contains("artifact parent"));
    assert!(!project.path().join("kvist.toml").exists());
}

#[test]
fn a_file_cannot_be_used_as_a_project_directory() {
    let workspace = TempDir::new().expect("create temporary workspace");
    let project_path = workspace.path().join("project");
    fs::write(&project_path, "not a directory").expect("create project file");

    let error = initialize(&project_path).expect_err("project path is a file");

    assert!(error.to_string().contains("must be a directory"));
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_cannot_be_used_as_a_project_directory() {
    use std::os::unix::fs::symlink;

    let workspace = TempDir::new().expect("create temporary workspace");
    let actual_project = workspace.path().join("actual-project");
    fs::create_dir(&actual_project).expect("create actual project");
    let project_link = workspace.path().join("project-link");
    symlink(&actual_project, &project_link).expect("create project link");

    let error = initialize(&project_link).expect_err("project link must be rejected");

    assert!(error.to_string().contains("symbolic link"));
    assert!(!actual_project.join("kvist.toml").exists());
}
