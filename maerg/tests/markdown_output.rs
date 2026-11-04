use std::process::Command;

fn run(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kvist"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

#[test]
fn ordinary_commands_emit_markdown_but_machine_routes_remain_native() {
    let dir = tempfile::tempdir().unwrap();
    let initialized = run(dir.path(), &["init"]);
    assert!(initialized.status.success());
    assert!(String::from_utf8_lossy(&initialized.stdout).starts_with("# "));
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success()
    );
    for args in [
        vec!["status"],
        vec!["doctor"],
        vec!["tree"],
        vec!["help"],
        vec!["task", "next", "."],
    ] {
        let output = run(dir.path(), &args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.starts_with("# "), "{args:?}: {text}");
        assert!(!text.contains('\x1b'), "{args:?}: {text}");
    }
    let text = run(dir.path(), &["status", "--format", "text"]);
    assert!(String::from_utf8_lossy(&text.stdout).starts_with("status-format-version: 1"));
    let json = run(dir.path(), &["status", "--format", "json"]);
    assert!(serde_json::from_slice::<serde_json::Value>(&json.stdout).is_ok());
    let global_json = run(dir.path(), &["--json", "status"]);
    assert_eq!(json.stdout, global_json.stdout);
    let completions = run(dir.path(), &["completions", "bash"]);
    assert!(String::from_utf8_lossy(&completions.stdout).starts_with("_kvist()"));
}

#[test]
fn status_markdown_has_structured_document_and_task_facts() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run(dir.path(), &["init"]).status.success());
    let status = run(dir.path(), &["status", "--details"]);
    let text = String::from_utf8(status.stdout).unwrap();
    assert!(text.contains("| Artifact | State |"), "{text}");
    assert!(text.contains("Task progress"), "{text}");
    assert!(text.contains("Next"), "{text}");
    assert!(!text.contains("╭──"), "{text}");
}

#[test]
fn incomplete_human_repair_is_markdown_and_explicit_json_stays_native() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("kvist.toml"),
        "schema_version = 1\ncomponent_root = \"src\"\n",
    )
    .unwrap();
    let human = run(dir.path(), &["repair", "--dry-run"]);
    assert!(!human.status.success());
    assert!(String::from_utf8_lossy(&human.stderr).starts_with("# Artifact Recovery\n\n"));
    let machine = run(dir.path(), &["--json", "repair", "--dry-run"]);
    assert!(!machine.status.success());
    assert!(serde_json::from_slice::<serde_json::Value>(&machine.stderr).is_ok());
}
