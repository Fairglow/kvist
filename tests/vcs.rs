use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

use kvist::init::initialize;
#[cfg(target_os = "linux")]
use kvist::{project_state::inspect, vcs::VcsArtifactState};
use tempfile::TempDir;

fn run_kvist(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(arguments)
        .output()
        .expect("run kvist")
}

fn run(program: &str, arguments: &[&str], directory: &Path) -> Output {
    Command::new(program)
        .args(arguments)
        .current_dir(directory)
        .output()
        .expect("run VCS command")
}

#[cfg(target_os = "linux")]
fn command_exists(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}

fn create_complete_component(path: &Path) {
    fs::create_dir_all(path).expect("create component");
    for name in ["SPEC.md", "TODOS.yaml", "DOCS.md"] {
        fs::write(path.join(name), "fixture").expect("write artifact");
    }
}

#[test]
fn doctor_reports_git_tracked_and_ignored_durable_artifacts_without_mutation() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    create_complete_component(&project.path().join("src/child"));
    fs::write(project.path().join(".gitignore"), "/src/DOCS.md\n").expect("write ignore rule");

    assert!(
        run("git", &["init", "--quiet"], project.path())
            .status
            .success()
    );
    assert!(
        run("git", &["add", "--all"], project.path())
            .status
            .success()
    );
    let before = run("git", &["status", "--porcelain=v1"], project.path()).stdout;

    let output = run_kvist(&[
        "doctor",
        project.path().to_str().expect("UTF-8 project path"),
    ]);

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("vcs: required durable artifacts need attention"));
    assert!(stdout.contains("vcs kvist.toml: tracked"));
    assert!(stdout.contains("vcs src/DOCS.md: ignored"));
    let nested_specification = Path::new("src").join("child").join("SPEC.md");
    assert!(stdout.contains(&format!("vcs {}: tracked", nested_specification.display())));
    assert_eq!(
        run("git", &["status", "--porcelain=v1"], project.path()).stdout,
        before
    );
}

#[test]
fn doctor_reports_when_no_supported_vcs_repository_contains_the_project() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");

    let output = run_kvist(&[
        "doctor",
        project.path().to_str().expect("UTF-8 project path"),
    ]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("vcs: no supported VCS repository found"));
}

#[test]
fn doctor_reports_a_malformed_git_repository_as_an_inspection_failure() {
    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    fs::create_dir(project.path().join(".git")).expect("create malformed Git directory");
    fs::write(project.path().join(".git/config"), "[core\n").expect("write malformed config");

    let output = run_kvist(&[
        "doctor",
        project.path().to_str().expect("UTF-8 project path"),
    ]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("vcs: inspection failed"));
    assert!(stdout.contains("vcs diagnostic: cannot inspect Git"));
}

#[cfg(target_os = "linux")]
#[test]
fn doctor_tracks_git_artifacts_in_non_utf8_component_paths() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    let component_name = OsString::from_vec(b"child-\xff".to_vec());
    let component = project.path().join("src").join(&component_name);
    create_complete_component(&component);
    assert!(
        run("git", &["init", "--quiet"], project.path())
            .status
            .success()
    );
    assert!(
        run("git", &["add", "--all"], project.path())
            .status
            .success()
    );

    let inspection = inspect(project.path()).expect("inspect");
    let expected_path = Path::new("src").join(component_name).join("SPEC.md");
    assert!(
        inspection.vcs.artifacts.iter().any(|artifact| {
            artifact.path == expected_path && artifact.state == VcsArtifactState::Tracked
        }),
        "non-UTF-8 component artifact should remain tracked"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn doctor_reports_jj_snapshot_tracking_when_jj_is_available() {
    if !command_exists("jj") {
        return;
    }

    let project = TempDir::new().expect("project");
    initialize(project.path()).expect("initialize");
    create_complete_component(&project.path().join("--components"));
    fs::write(
        project.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"--components\"\n[vcs]\nkind = \"jj\"\n",
    )
    .expect("select jj");
    assert!(
        run("jj", &["git", "init", "--no-colocate", "."], project.path())
            .status
            .success()
    );
    assert!(
        run("jj", &["file", "list", "-r", "@"], project.path())
            .status
            .success()
    );
    assert!(
        run(
            "jj",
            &[
                "config",
                "set",
                "--repo",
                "templates.file_list",
                "\"\\\"decorated\\\"\"",
            ],
            project.path(),
        )
        .status
        .success()
    );
    fs::write(project.path().join("dirty.txt"), "not snapshotted").expect("write dirty file");
    let before_operation = run(
        "jj",
        &["--ignore-working-copy", "op", "log", "-T", "id", "-n", "1"],
        project.path(),
    )
    .stdout;

    let output = run_kvist(&[
        "doctor",
        project.path().to_str().expect("UTF-8 project path"),
    ]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("vcs: all required durable artifacts are tracked"));
    assert!(stdout.contains("vcs src/SPEC.md: tracked"));
    assert!(stdout.contains("vcs --components/SPEC.md: tracked"));
    assert!(stdout.contains("vcs diagnostic: jj inspection uses its saved working-copy snapshot"));
    assert_eq!(
        run(
            "jj",
            &["--ignore-working-copy", "op", "log", "-T", "id", "-n", "1"],
            project.path(),
        )
        .stdout,
        before_operation
    );
    assert!(
        run(
            "jj",
            &[
                "--ignore-working-copy",
                "file",
                "list",
                "-r",
                "@",
                "-T",
                "path",
                "root:\"dirty.txt\"",
            ],
            project.path(),
        )
        .stdout
        .is_empty()
    );
}
