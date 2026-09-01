use std::{
    fs,
    process::{Command, Output},
};

use kvist::{
    artifacts::{
        ARCHITECTURE_VERSION, CONFIGURATION_VERSION, IMPLEMENTATION_RECORD_VERSION,
        ROOT_CONTRACT_VERSION, TODO_QUEUE_VERSION, VISION_VERSION,
    },
    component_documents::{CONTRACT_VERSION, DESIGN_VERSION, REQUIREMENTS_VERSION},
    init::initialize,
    project_state::{MAX_ROOT_TEXT_ARTIFACT_BYTES, ProjectState, inspect},
};
use tempfile::TempDir;

fn run_kvist(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .output()
        .expect("run kvist command")
}

#[test]
fn doctor_reports_state_diagnostics_without_writing() {
    let project = TempDir::new().expect("project");
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"src\"\n",
    )
    .expect("write partial artifact");

    let before = fs::read_to_string(project.path().join("kvist.toml")).expect("read before");
    let output = run_kvist(&[
        "doctor",
        project.path().to_str().expect("UTF-8 project path"),
    ]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("state: partial"));
    assert!(stdout.contains("ROOT_CONTRACT.md: missing"));
    assert!(stdout.contains("Phase 1 never repairs or migrates artifacts automatically"));
    assert_eq!(
        fs::read_to_string(project.path().join("kvist.toml")).expect("read after"),
        before
    );
}

#[test]
fn inspection_honors_an_arbitrary_configured_component_root() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::rename(
        project.path().join("src"),
        project.path().join("components"),
    )
    .expect("move component root");
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"components\"\n",
    )
    .expect("configure component root");

    let inspection = inspect(project.path()).expect("inspect");

    assert_eq!(inspection.state, ProjectState::Current);
    assert_eq!(
        inspection.component_root.as_deref(),
        Some(std::path::Path::new("components"))
    );
    assert!(
        inspection
            .artifacts
            .iter()
            .any(|artifact| artifact.path == "components/REQUIREMENTS.md")
    );
    assert!(
        inspection
            .artifacts
            .iter()
            .all(|artifact| !artifact.path.starts_with("src/"))
    );
}

#[test]
fn init_refuses_partial_invalid_and_unsupported_projects_without_overwriting() {
    let partial = TempDir::new().expect("partial project");
    let partial_config = partial.path().join("kvist.toml");
    fs::write(
        &partial_config,
        "schema_version = 1\ncomponent_root = \"src\"\n",
    )
    .expect("write");
    let partial_before = fs::read_to_string(&partial_config).expect("read");
    let error = initialize(partial.path()).expect_err("partial must be refused");
    assert!(error.to_string().contains("state is partial"));
    assert_eq!(
        fs::read_to_string(&partial_config).expect("read"),
        partial_before
    );

    let invalid = TempDir::new().expect("invalid project");
    initialize(invalid.path()).expect("initialize");
    let contract = invalid.path().join("ROOT_CONTRACT.md");
    fs::write(&contract, "invalid").expect("corrupt contract");
    let invalid_before = fs::read_to_string(&contract).expect("read");
    let error = initialize(invalid.path()).expect_err("invalid must be refused");
    assert!(error.to_string().contains("state is invalid"));
    assert_eq!(fs::read_to_string(&contract).expect("read"), invalid_before);

    let unsupported = TempDir::new().expect("unsupported project");
    initialize(unsupported.path()).expect("initialize");
    let todos = unsupported.path().join("src/TODOS.yaml");
    fs::write(&todos, "schema_version: 99\ntasks: []\n").expect("write unsupported version");
    let unsupported_before = fs::read_to_string(&todos).expect("read");
    let error = initialize(unsupported.path()).expect_err("unsupported must be refused");
    assert!(error.to_string().contains("state is unsupported-version"));
    assert_eq!(
        fs::read_to_string(&todos).expect("read"),
        unsupported_before
    );
}

#[test]
fn invalid_contents_and_artifact_types_are_classified_as_invalid() {
    let cases = [
        (
            "kvist.toml",
            "schema_version = \"one\"\ncomponent_root = \"src\"\n",
        ),
        ("ROOT_CONTRACT.md", "# Kvist Root Contract\n"),
        (
            "src/REQUIREMENTS.md",
            "<!-- kvist-requirements-version: 1 -->\n",
        ),
        ("src/CONTRACT.md", "<!-- kvist-contract-version: 1 -->\n"),
        ("src/DESIGN.md", "<!-- kvist-design-version: 1 -->\n"),
        (
            "src/TODOS.yaml",
            "schema_version: 1\ncomponent: invalid\ntasks: []\n",
        ),
        (
            "src/IMPL.md",
            "<!-- kvist-implementation-record-version: 1 -->\n",
        ),
    ];

    for (path, contents) in cases {
        let project = TempDir::new().expect("project");
        initialize(project.path()).expect("initialize");
        fs::write(project.path().join(path), contents).expect("corrupt artifact");
        assert_eq!(
            inspect(project.path()).expect("inspect").state,
            ProjectState::Invalid
        );
    }

    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    let contract = project.path().join("ROOT_CONTRACT.md");
    fs::remove_file(&contract).expect("remove contract");
    fs::create_dir(&contract).expect("replace contract with directory");
    assert_eq!(
        inspect(project.path()).expect("inspect").state,
        ProjectState::Invalid
    );
}

#[test]
fn every_version_domain_can_report_an_unsupported_version() {
    let cases = [
        (
            "kvist.toml",
            "schema_version = 99\ncomponent_root = \"src\"\n",
        ),
        (
            "ROOT_CONTRACT.md",
            "<!-- kvist-root-contract-version: 99 -->\n# Kvist Root Contract\n",
        ),
        (
            "VISION.md",
            "<!-- kvist-vision-version: 99 -->\n# Project Vision\n",
        ),
        (
            "ARCHITECTURE.md",
            "<!-- kvist-architecture-version: 99 -->\n# Project Architecture\n",
        ),
        (
            "src/REQUIREMENTS.md",
            "<!-- kvist-requirements-version: 99 -->\n# Component Requirements\n",
        ),
        (
            "src/CONTRACT.md",
            "<!-- kvist-contract-version: 99 -->\n# Component Contract\n",
        ),
        (
            "src/DESIGN.md",
            "<!-- kvist-design-version: 99 -->\n# Component Design\n",
        ),
        (
            "src/TODOS.yaml",
            "schema_version: 99\ntasks:\n  - id: task\n    status: pending\n    description: task\n",
        ),
        (
            "src/IMPL.md",
            "<!-- kvist-implementation-record-version: 99 -->\n# Component Implementation Record\n",
        ),
    ];

    for (path, contents) in cases {
        let project = TempDir::new().expect("project");
        initialize(project.path()).expect("initialize");
        fs::write(project.path().join(path), contents).expect("write unsupported version");
        assert_eq!(
            inspect(project.path()).expect("inspect").state,
            ProjectState::UnsupportedVersion
        );
    }
}

#[test]
fn version_domains_are_independent_public_constants() {
    assert_eq!(CONFIGURATION_VERSION, 1);
    assert_eq!(VISION_VERSION, 1);
    assert_eq!(ARCHITECTURE_VERSION, 1);
    assert_eq!(ROOT_CONTRACT_VERSION, 1);
    assert_eq!(REQUIREMENTS_VERSION, 1);
    assert_eq!(CONTRACT_VERSION, 1);
    assert_eq!(DESIGN_VERSION, 1);
    assert_eq!(TODO_QUEUE_VERSION, 1);
    assert_eq!(IMPLEMENTATION_RECORD_VERSION, 1);
}

#[test]
fn oversized_or_invalid_utf8_root_artifacts_remain_inspectable_as_invalid() {
    for path in [
        "ROOT_CONTRACT.md",
        "VISION.md",
        "ARCHITECTURE.md",
        "src/REQUIREMENTS.md",
        "src/CONTRACT.md",
        "src/DESIGN.md",
        "src/TODOS.yaml",
        "src/IMPL.md",
    ] {
        let project = TempDir::new().expect("project");
        initialize(project.path()).expect("initialize");
        fs::write(
            project.path().join(path),
            vec![b'x'; MAX_ROOT_TEXT_ARTIFACT_BYTES as usize + 1],
        )
        .expect("write oversized artifact");

        assert_eq!(
            inspect(project.path())
                .expect("inspect oversized artifact")
                .state,
            ProjectState::Invalid
        );
    }

    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("ROOT_CONTRACT.md"), [0xff, 0xfe]).expect("write invalid UTF-8");

    assert_eq!(
        inspect(project.path())
            .expect("inspect invalid UTF-8")
            .state,
        ProjectState::Invalid
    );
}

#[test]
fn non_positive_configuration_versions_are_invalid_not_unsupported() {
    for version in [0, -1] {
        let project = TempDir::new().expect("project");
        initialize(project.path()).expect("initialize");
        fs::write(
            project.path().join("kvist.toml"),
            format!("schema_version = {version}\ncomponent_root = \"src\"\n"),
        )
        .expect("write invalid configuration version");

        assert_eq!(
            inspect(project.path())
                .expect("inspect invalid version")
                .state,
            ProjectState::Invalid
        );
    }
}
