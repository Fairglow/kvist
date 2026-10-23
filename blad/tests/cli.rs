use std::process::Command;

fn blad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_blad"))
}

#[test]
fn help_and_version_work_without_opening_files_or_themes() {
    for flag in ["--version", "-V"] {
        let output = blad()
            .args([flag, "--theme-file", "/missing/theme.json"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            concat!("blad ", env!("CARGO_PKG_VERSION"))
        );
    }
    let output = blad().arg("--help").output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for option in [
        "--dump",
        "--theme",
        "--theme-file",
        "--color",
        "--math",
        "--mermaid",
        "--diagram-charset",
        "--line-numbers",
        "--version",
    ] {
        assert!(help.contains(option), "{help}");
    }
    assert_eq!(
        help.contains("--log-dir"),
        cfg!(feature = "sessions"),
        "{help}"
    );
    assert_eq!(
        help.contains("Commands:"),
        cfg!(feature = "sessions"),
        "{help}"
    );
    if cfg!(feature = "sessions") {
        assert!(
            help.contains("session") && help.contains("export") && help.contains("list"),
            "{help}"
        );
    }
    assert!(
        !help.lines().any(|line| ["show", "view", "stream"]
            .iter()
            .any(|name| line.trim_start().starts_with(&format!("{name} ")))),
        "{help}"
    );
    let empty = blad().output().unwrap();
    assert!(!empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stderr).contains("Usage:"));
}

#[test]
fn file_arguments_are_extension_independent_ordered_and_deduplicated() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second.txt");
    std::fs::write(&first, "# First").unwrap();
    std::fs::write(&second, "# Second").unwrap();
    let output = blad()
        .arg("--dump")
        .arg(&first)
        .arg(&second)
        .arg(&first)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "# First\n# Second\n"
    );

    let missing = blad()
        .arg("--dump")
        .arg(&first)
        .arg(temp.path().join("missing"))
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(
        missing.stdout.is_empty(),
        "all initial files are loaded before output"
    );
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing"));
}

#[test]
fn filenames_matching_commands_are_unambiguous_after_double_dash() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["session", "list", "export", "show", "view", "stream"] {
        std::fs::write(temp.path().join(name), format!("# {name}")).unwrap();
        let output = blad()
            .current_dir(temp.path())
            .args(["--dump", "--", name])
            .output()
            .unwrap();
        assert!(output.status.success(), "{name}: {:?}", output.stderr);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            format!("# {name}")
        );
    }

    if !cfg!(feature = "sessions") {
        let output = blad().args(["--log-dir", "/missing"]).output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
    }
}

#[cfg(feature = "sessions")]
#[test]
fn global_options_do_not_hide_commands_and_files_cannot_be_mixed_with_them() {
    let output = blad()
        .args(["--dump", "session", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("<SESSION_ID>"));

    let output = blad().args(["session", "--version"]).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));

    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("first.md"), "# First").unwrap();
    let output = blad()
        .current_dir(temp.path())
        .args(["--dump", "first.md", "session", "missing"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be combined"));

    let output = blad()
        .arg("--log-dir")
        .arg(temp.path().join("missing-\x1b[2J"))
        .args(["--dump", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("No sessions found"));
    assert!(!output.stdout.contains(&0x1b));
}

#[cfg(unix)]
#[test]
fn file_arguments_preserve_non_utf8_paths() {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let file = temp
        .path()
        .join(std::ffi::OsString::from_vec(b"file-\xff.md".to_vec()));
    std::fs::write(&file, "# Non-UTF-8 filename").unwrap();
    let output = blad().arg("--dump").arg(&file).output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "# Non-UTF-8 filename"
    );
}
