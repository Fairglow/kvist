use std::process::Command;

fn blad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_blad"))
}

fn output_before_deadline(command: &mut Command) -> std::process::Output {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("file input did not finish before the deadline: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn oversized_initial_files_fail_before_output_and_before_utf8_decoding() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.md");
    let oversized = temp.path().join("oversized.md");
    std::fs::write(&first, "# First").unwrap();
    std::fs::write(&oversized, [0xff]).unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&oversized)
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    let output = output_before_deadline(blad().arg("--dump").arg(&first).arg(&oversized));
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("16777216") && error.contains("oversized.md"),
        "{error}"
    );
}

#[test]
fn special_initial_files_fail_without_waiting_for_a_fifo_writer() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.md");
    let fifo = temp.path().join("fifo-\x1b[2J");
    std::fs::write(&first, "# First").unwrap();
    nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).unwrap();
    for input in [&fifo, &temp.path().to_path_buf()] {
        let output = output_before_deadline(blad().arg("--dump").arg(&first).arg(input));
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("requires a regular file"));
        assert!(!output.stderr.contains(&0x1b));
    }
}

#[test]
fn special_theme_files_fail_without_waiting_for_a_fifo_writer() {
    let temp = tempfile::tempdir().unwrap();
    let doc = temp.path().join("doc.md");
    let fifo = temp.path().join("theme-\x1b[2J");
    std::fs::write(&doc, "# Document").unwrap();
    nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).unwrap();
    for theme in [&fifo, &temp.path().to_path_buf()] {
        let output = output_before_deadline(
            blad()
                .arg("--theme-file")
                .arg(theme)
                .arg("--dump")
                .arg(&doc),
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("requires a regular file"));
        assert!(!output.stderr.contains(&0x1b));
    }
}

#[test]
fn authored_symlinks_remain_supported_for_documents_and_themes() {
    let temp = tempfile::tempdir().unwrap();
    let doc = temp.path().join("doc.md");
    let theme = temp.path().join("theme.json");
    let doc_link = temp.path().join("doc-link.md");
    let theme_link = temp.path().join("theme-link.json");
    std::fs::write(&doc, "# Authored symlinks").unwrap();
    std::fs::write(&theme, "{}").unwrap();
    std::os::unix::fs::symlink(&doc, &doc_link).unwrap();
    std::os::unix::fs::symlink(&theme, &theme_link).unwrap();
    let output = output_before_deadline(
        blad()
            .arg("--theme-file")
            .arg(&theme_link)
            .arg("--dump")
            .arg(&doc_link)
            .arg(&doc),
    );
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(output.stdout, b"# Authored symlinks\n");
}

#[test]
fn invalid_utf8_input_fails_without_partial_dump_output() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.md");
    let invalid = temp.path().join("invalid.md");
    std::fs::write(&first, "# First").unwrap();
    std::fs::write(&invalid, [b'x', 0xff]).unwrap();
    let output = output_before_deadline(blad().arg("--dump").arg(&first).arg(&invalid));
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .to_ascii_lowercase()
            .contains("utf-8")
    );
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
