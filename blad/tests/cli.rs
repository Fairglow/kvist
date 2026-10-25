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
    let pty = nix::pty::openpty(None, None).unwrap();
    let empty = blad().stdin(pty.slave).output().unwrap();
    assert!(!empty.status.success());
    assert!(String::from_utf8_lossy(&empty.stderr).contains("Usage:"));
}

#[test]
fn stdin_documents_are_bounded_ordered_and_rendered_once_without_terminal_controls() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("stdin.md");
    let named = temp.path().join("named.md");
    std::fs::write(&input, "# Piped\n\n**bold** and $x^2$\n").unwrap();
    std::fs::write(&named, "# Named").unwrap();
    for args in [vec![], vec!["-"], vec!["--dump", "-"]] {
        let output = blad()
            .args(args)
            .stdin(std::fs::File::open(&input).unwrap())
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains("# Piped") && text.contains("bold and x²"),
            "{text}"
        );
        assert!(!text.contains('\x1b'), "{text:?}");
    }
    let output = blad()
        .arg(&named)
        .args(["-", "-"])
        .arg(&named)
        .stdin(std::fs::File::open(&input).unwrap())
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("# Named\n# Piped"), "{text}");
    assert_eq!(text.matches("# Named").count(), 1);
    assert_eq!(text.matches("# Piped").count(), 1);
    for bytes in [vec![0xff], vec![b'x'; 16 * 1024 * 1024 + 1]] {
        std::fs::write(&input, bytes).unwrap();
        let output = blad()
            .arg(&named)
            .arg("-")
            .stdin(std::fs::File::open(&input).unwrap())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    let output = blad().stdin(std::process::Stdio::null()).output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stdout.is_empty());
}

#[test]
fn redirected_file_output_is_plain_by_default_and_head_closes_quietly() {
    use std::process::Stdio;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("many.md");
    std::fs::write(&path, "**rendered output**\n\n".repeat(10_000)).unwrap();
    let mut producer = blad()
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut head = Command::new("head")
        .args(["-n", "30"])
        .stdin(Stdio::from(producer.stdout.take().unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if producer.try_wait().unwrap().is_some() && head.try_wait().unwrap().is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = producer.kill();
            let _ = head.kill();
            let _ = producer.wait();
            let _ = head.wait();
            panic!("pipeline did not exit before deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let producer = producer.wait_with_output().unwrap();
    let head = head.wait_with_output().unwrap();
    assert!(producer.status.success(), "{:?}", producer.stderr);
    assert!(producer.stderr.is_empty(), "{:?}", producer.stderr);
    assert!(head.status.success());
    assert_eq!(
        head.stdout.iter().filter(|byte| **byte == b'\n').count(),
        30
    );
    assert!(!head.stdout.contains(&0x1b));
}

#[test]
fn real_stdin_pipe_and_inclusive_limit_finish_as_finite_snapshots() {
    use std::process::Stdio;
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input");
    std::fs::write(&input, "# From a pipe\n\n**bold**").unwrap();
    let mut cat = Command::new("cat")
        .arg(&input)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let output = blad()
        .stdin(Stdio::from(cat.stdout.take().unwrap()))
        .output()
        .unwrap();
    assert!(cat.wait().unwrap().success());
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(output.stdout, b"# From a pipe\n\nbold\n");
    std::fs::write(&input, vec![b' '; 16 * 1024 * 1024]).unwrap();
    let output = blad()
        .stdin(std::fs::File::open(&input).unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn redirected_stdout_with_terminal_stdin_does_not_start_a_viewer() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input");
    std::fs::write(&input, "# Named\n\n**bold**").unwrap();
    let pty = nix::pty::openpty(None, None).unwrap();
    let mut child = blad()
        .arg(input)
        .stdin(pty.slave)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("redirected output entered the viewer");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(output.stdout, b"# Named\n\nbold\n");
}

#[test]
fn non_pipe_stdout_errors_remain_failures() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("input");
    std::fs::write(&input, "# Full output").unwrap();
    let output = blad()
        .arg(&input)
        .stdout(
            std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/full")
                .unwrap(),
        )
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Failed to display markdown"));
}

#[cfg(feature = "sessions")]
#[test]
fn empty_session_list_handles_a_fully_closed_stdout_without_panicking() {
    let temp = tempfile::tempdir().unwrap();
    let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
    drop(reader);
    let output = blad()
        .args(["list", "--log-dir"])
        .arg(temp.path())
        .stdout(std::process::Stdio::from(std::os::fd::OwnedFd::from(
            writer,
        )))
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
}

#[test]
fn explicit_stdin_on_a_terminal_is_rejected_without_waiting() {
    let pty = nix::pty::openpty(None, None).unwrap();
    let mut child = blad()
        .arg("-")
        .stdin(pty.slave)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("explicit terminal stdin waited for input");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("redirected stdin"));
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

#[cfg(feature = "sessions")]
fn session_fixture(root: &std::path::Path, id: &str) -> std::path::PathBuf {
    let session = root.join(id);
    std::fs::create_dir_all(session.join("messages")).unwrap();
    std::fs::write(session.join("messages/one.md"), "message body").unwrap();
    let manifest = serde_json::json!({
        "schema_version": 1,
        "kind": "skott-conversation",
        "session_id": id,
        "status": "completed",
        "created_at": "2026-10-24T10:00:00Z",
        "entries": [{"sequence": 1, "kind": "user", "path": "messages/one.md"}]
    });
    std::fs::write(
        session.join("session.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    session
}

#[cfg(feature = "sessions")]
#[test]
fn malformed_session_artifacts_fail_explicitly_before_output() {
    for invalid in [b"invalid json".as_slice(), b"{}".as_slice()] {
        let temp = tempfile::tempdir().unwrap();
        let path = session_fixture(temp.path(), "session");
        std::fs::write(path.join("session.json"), invalid).unwrap();
        for args in [["list", ""], ["session", "session"], ["export", "session"]] {
            let mut command = blad();
            command
                .arg("--log-dir")
                .arg(temp.path())
                .arg("--dump")
                .arg(args[0]);
            if !args[1].is_empty() {
                command.arg(args[1]);
            }
            let output = output_before_deadline(&mut command);
            assert_eq!(output.status.code(), Some(1), "{:?}", output.stdout);
            assert!(output.stdout.is_empty());
            assert!(!output.stderr.is_empty());
        }
    }
}

#[cfg(feature = "sessions")]
#[test]
fn failed_message_reads_do_not_become_empty_successful_transcripts() {
    let temp = tempfile::tempdir().unwrap();
    let path = session_fixture(temp.path(), "session");
    std::fs::remove_file(path.join("messages/one.md")).unwrap();
    for command in ["session", "export"] {
        let output = output_before_deadline(
            blad()
                .arg("--log-dir")
                .arg(temp.path())
                .args(["--dump", command, "session"]),
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("one.md"));
    }
}

#[cfg(feature = "sessions")]
#[test]
fn session_artifact_symlinks_and_nonregular_inputs_are_rejected() {
    for target in ["session", "session.json", "messages", "messages/one.md"] {
        let temp = tempfile::tempdir().unwrap();
        let path = session_fixture(temp.path(), "session");
        let candidate = if target == "session" {
            path.clone()
        } else {
            path.join(target)
        };
        let saved = temp.path().join("saved");
        std::fs::rename(&candidate, &saved).unwrap();
        std::os::unix::fs::symlink(&saved, &candidate).unwrap();
        let output = output_before_deadline(blad().arg("--dump").arg("session").arg(&path));
        assert_eq!(output.status.code(), Some(1), "followed {target}");
        assert!(output.stdout.is_empty(), "{target}: {:?}", output.stdout);
    }
    let temp = tempfile::tempdir().unwrap();
    let path = session_fixture(temp.path(), "session");
    std::fs::remove_file(path.join("messages/one.md")).unwrap();
    nix::unistd::mkfifo(&path.join("messages/one.md"), nix::sys::stat::Mode::S_IRUSR).unwrap();
    let output = output_before_deadline(blad().args(["--dump", "session"]).arg(&path));
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[cfg(feature = "sessions")]
#[test]
fn session_paths_and_ambiguous_prefixes_fail_explicitly() {
    for entry in [
        "../outside.md",
        "/etc/passwd",
        "messages/../outside.md",
        "messages/nested/file.md",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = session_fixture(temp.path(), "session");
        let manifest_path = path.join("session.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["entries"][0]["path"] = entry.into();
        std::fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let output = output_before_deadline(blad().args(["--dump", "session"]).arg(&path));
        assert_eq!(output.status.code(), Some(1), "{entry}");
        assert!(output.stdout.is_empty());
    }

    let temp = tempfile::tempdir().unwrap();
    session_fixture(temp.path(), "prefix-a");
    session_fixture(temp.path(), "prefix-b");
    for id in ["prefix", ""] {
        let output = output_before_deadline(
            blad()
                .arg("--log-dir")
                .arg(temp.path())
                .args(["--dump", "session", id]),
        );
        assert_eq!(output.status.code(), Some(1), "{id:?}");
        assert!(output.stdout.is_empty());
    }
}

#[cfg(feature = "sessions")]
#[test]
fn session_roots_reject_symlinks_even_with_a_trailing_slash() {
    let temp = tempfile::tempdir().unwrap();
    let session = session_fixture(temp.path(), "session");
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&session, &link).unwrap();
    let target = format!("{}/", link.display());
    let output = output_before_deadline(blad().args(["--dump", "session", &target]));
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}
