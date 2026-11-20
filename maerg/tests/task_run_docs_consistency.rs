//! Documentation consistency for `kvist task run`.
//!
//! The provided-interfaces contract (`CONTRACT.md`, "Provided interfaces")
//! fixes three surfaces of the `task run` command that the user-facing
//! documentation must describe the same way:
//!
//! - the target is optional, and a bare positive integer `N` runs the Nth
//!   ready task (1-based, the same ordering the `tasks` listing numbers), so
//!   the CLI surface is `[TASK_ID|N]`;
//! - without a target the command executes ready tasks continuously, without
//!   confirmation;
//! - a protected run requires the complete effective execution policy to have
//!   been approved with `kvist task approve-policy`.
//!
//! These tests fail while the README CLI table or the GUIDE task-run examples
//! contradict those contracted surfaces, and pass once they agree. They read
//! the documents only; they do not execute a task.

use std::fs;
use std::path::PathBuf;

/// The maerg component directory, where the component README lives.
fn component_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The workspace root holds the project-level `GUIDE.md`.
fn workspace_root() -> PathBuf {
    component_dir().parent().expect("maerg has a parent directory").to_path_buf()
}

/// The single CLI-table row that documents `kvist task run`.
fn readme_task_run_row(readme: &str) -> &str {
    readme
        .lines()
        .find(|line| line.contains("kvist task run"))
        .expect("README CLI table is missing its `kvist task run` row")
}

/// The GUIDE section that shows the task-run examples.
fn guide_task_run_section(guide: &str) -> &str {
    let start = guide
        .find("### 5.2 Running a Task")
        .expect("GUIDE is missing its task-run examples section");
    let tail = &guide[start..];
    let end = tail
        .find("### 5.3")
        .unwrap_or(tail.len());
    &tail[..end]
}

/// The README CLI table and the GUIDE task-run examples both describe the
/// optional target with the numbered ready-task shortcut, matching the
/// contracted `[TASK_ID|N]` surface.
#[test]
fn task_run_target_is_optional_with_numbered_shortcut() {
    let readme =
        fs::read_to_string(component_dir().join("README.md")).expect("read the component README");
    let row = readme_task_run_row(&readme);
    assert!(
        row.contains("[TASK_ID|N]"),
        "README CLI table row must show the optional numbered target `[TASK_ID|N]` \
         (a bare positive integer runs the Nth ready task), not a plain `[TASK_ID]`; \
         row: {row}"
    );

    let guide =
        fs::read_to_string(workspace_root().join("GUIDE.md")).expect("read the project GUIDE");
    let section = guide_task_run_section(&guide);
    assert!(
        section.contains("ready task"),
        "GUIDE task-run examples must document the numbered ready-task target \
         (a bare positive integer runs the Nth ready task), matching the contracted \
         `[TASK_ID|N]` surface; section: {section}"
    );
}

/// Continuous execution (no target) is described as running without another
/// confirmation, matching the contract and `REQ-INTERACTIVE-SHELL`.
#[test]
fn continuous_task_run_needs_no_confirmation() {
    let guide =
        fs::read_to_string(workspace_root().join("GUIDE.md")).expect("read the project GUIDE");
    let section = guide_task_run_section(&guide);
    assert!(
        section.contains("without confirmation"),
        "GUIDE task-run examples must state that omitting the target runs \
         continuously without confirmation; section: {section}"
    );
}

/// Both user-facing documents tie task execution to the approved execution
/// policy recorded by `kvist task approve-policy`.
#[test]
fn task_run_requires_approved_policy() {
    let readme =
        fs::read_to_string(component_dir().join("README.md")).expect("read the component README");
    assert!(
        readme.contains("kvist task approve-policy"),
        "README must document the `kvist task approve-policy` requirement for task execution"
    );

    let guide =
        fs::read_to_string(workspace_root().join("GUIDE.md")).expect("read the project GUIDE");
    assert!(
        guide.contains("approve-policy"),
        "GUIDE must require the execution policy to be approved with \
         `kvist task approve-policy` before task execution"
    );
}
