//! Build-script inputs must not invalidate an unchanged Cargo invocation.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn checked(command: &mut Command) -> Output {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn run_metadata(program: &Path, workdir: &Path) -> String {
    let output = checked(
        Command::new(program)
            .current_dir(workdir)
            .env("SOURCE_DATE_EPOCH", "0"),
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let mut watches = 0;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("cargo:rerun-if-changed=") {
            let path = workdir.join(path);
            assert!(
                path.exists(),
                "nonexistent Cargo rebuild input: {}",
                path.display()
            );
            watches += 1;
        }
    }
    assert!(watches > 0, "Cargo needs explicit existing rebuild inputs");
    assert!(text.contains("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH"));
    assert!(text.contains("cargo:rustc-env=BUILD_DATE=1970-01-01 00:00:00 UTC"));
    text
}

#[test]
fn metadata_watches_existing_git_inputs_tracked_edits_worktrees_and_exports() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let repository = root.join("repository");
    fs::create_dir_all(repository.join("skott")).unwrap();
    fs::write(repository.join("tracked.txt"), "clean\n").unwrap();
    fs::write(repository.join("skott/build.rs"), "// fixture\n").unwrap();
    checked(Command::new("git").args(["init", "-q"]).arg(&repository));
    checked(
        Command::new("git")
            .current_dir(&repository)
            .args(["add", "."]),
    );
    checked(Command::new("git").current_dir(&repository).args([
        "-c",
        "user.name=Metadata test",
        "-c",
        "user.email=metadata@example.invalid",
        "commit",
        "-qm",
        "fixture",
    ]));
    let program = root.join("metadata-probe");
    checked(
        Command::new("rustc")
            .args(["--edition=2024", "--crate-name=metadata_probe"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs"))
            .arg("-o")
            .arg(&program),
    );
    let text = run_metadata(&program, &repository.join("skott"));
    assert!(text.contains("cargo:rustc-env=GIT_DIRTY=false"));
    assert!(text.contains(&format!(
        "cargo:rerun-if-changed={}",
        repository.join("tracked.txt").display()
    )));
    fs::write(repository.join("tracked.txt"), "dirty\n").unwrap();
    let text = run_metadata(&program, &repository.join("skott"));
    assert!(text.contains("cargo:rustc-env=GIT_DIRTY=true"));
    let linked = root.join("linked-worktree");
    checked(
        Command::new("git")
            .current_dir(&repository)
            .args(["worktree", "add", "--detach", "-q"])
            .arg(&linked),
    );
    let text = run_metadata(&program, &linked.join("skott"));
    assert!(text.contains("cargo:rustc-env=GIT_DIRTY=false"));
    assert!(text.contains(".git/worktrees/"));
    let export = root.join("export");
    fs::create_dir(&export).unwrap();
    fs::write(export.join("build.rs"), "// fixture\n").unwrap();
    let text = run_metadata(&program, &export);
    assert!(text.contains("cargo:rustc-env=GIT_SHA=unknown"));
}
