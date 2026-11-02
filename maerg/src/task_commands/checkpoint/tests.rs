use super::*;
use std::{
    io::{BufRead, Seek, SeekFrom},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::Command,
    time::{Duration, Instant},
};

struct Repo {
    directory: tempfile::TempDir,
    context: TaskContext,
}

impl Repo {
    fn new() -> Self {
        Self::with_format("sha1")
    }

    fn with_format(format: &str) -> Self {
        let directory = tempfile::Builder::new()
            .prefix(".checkpoint-test-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("component/src")).unwrap();
        fs::create_dir_all(root.join("component/tests")).unwrap();
        fs::write(root.join("component/src/main.rs"), b"before\n").unwrap();
        fs::write(root.join("component/TODOS.yaml"), b"queue\n").unwrap();
        fs::write(root.join("unrelated"), b"original\n").unwrap();
        let context = TaskContext {
            project_dir: root.to_owned(),
            component_dir: root.join("component"),
            component_path: PathBuf::from("component"),
        };
        let repo = Self { directory, context };
        for args in [
            vec!["init", "-q", "--object-format", format],
            vec!["config", "user.name", "Checkpoint Test"],
            vec!["config", "user.email", "checkpoint@example.invalid"],
            vec!["config", "commit.gpgsign", "false"],
            vec!["add", "."],
            vec!["commit", "-qm", "initial"],
        ] {
            repo.git(&args);
        }
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.directory.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn write(&self, path: &str, content: &str) {
        fs::write(self.directory.path().join(path), content).unwrap();
    }

    fn pending(&self) -> PreparedCommit {
        let before = capture(&self.context).unwrap();
        let head = ensure_clean(&self.context, &before).unwrap();
        self.write("component/src/main.rs", "after\n");
        let after = capture(&self.context).unwrap();
        prepare(&self.context, &head, &before, &after, "checkpoint").unwrap()
    }
}

#[test]
fn checkpoint_sha256_repository_deletions_are_exact() {
    let repo = Repo::with_format("sha256");
    let before = capture(&repo.context).unwrap();
    let head = ensure_clean(&repo.context, &before).unwrap();
    fs::remove_file(repo.context.component_dir.join("src/main.rs")).unwrap();
    let after = capture(&repo.context).unwrap();
    let pending = prepare(&repo.context, &head, &before, &after, "deletion").unwrap();
    assert_eq!(pending.commit_id.len(), 64);
    finish(&repo.context, &pending).unwrap();
    ensure_clean(&repo.context, &after).unwrap();
}

#[test]
fn checkpoint_consecutive_commits_preserve_unrelated_staging_and_worktree() {
    let repo = Repo::new();
    repo.write("unrelated", "staged\n");
    repo.git(&["add", "unrelated"]);
    repo.write("unrelated", "unstaged\n");
    repo.write("untracked", "never touched\n");
    for value in ["first\n", "second\n"] {
        let before = capture(&repo.context).unwrap();
        let head = ensure_clean(&repo.context, &before).unwrap();
        repo.write("component/src/main.rs", value);
        let after = capture(&repo.context).unwrap();
        let pending = prepare(&repo.context, &head, &before, &after, "item done").unwrap();
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(finish(&repo.context, &pending).unwrap(), pending.commit_id);
        assert_eq!(repo.git(&["show", ":unrelated"]), "staged");
        assert_eq!(repo.git(&["show", "HEAD:unrelated"]), "original");
        assert_eq!(
            fs::read_to_string(repo.directory.path().join("unrelated")).unwrap(),
            "unstaged\n"
        );
        assert_eq!(
            fs::read_to_string(repo.directory.path().join("untracked")).unwrap(),
            "never touched\n"
        );
        assert!(repo.git(&["log", "-1", "--format=%B"]).contains(COAUTHOR));
    }
    assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]), "3");
}

#[test]
fn checkpoint_preserves_executable_mode_and_deletion_and_literal_names() {
    let repo = Repo::new();
    let before = capture(&repo.context).unwrap();
    let head = ensure_clean(&repo.context, &before).unwrap();
    fs::remove_file(repo.context.component_dir.join("src/main.rs")).unwrap();
    let literal = "component/src/:(glob)* [literal]\\\n.rs";
    repo.write(literal, "executable\n");
    fs::set_permissions(
        repo.directory.path().join(literal),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let after = capture(&repo.context).unwrap();
    let pending = prepare(&repo.context, &head, &before, &after, "modes").unwrap();
    finish(&repo.context, &pending).unwrap();
    assert!(repo.git(&["ls-tree", "-r", "HEAD"]).contains("100755"));
    assert!(!repo.git(&["ls-tree", "-r", "HEAD"]).contains("main.rs"));
    assert_eq!(capture(&repo.context).unwrap(), after);
    ensure_clean(&repo.context, &after).unwrap();
}

#[test]
fn checkpoint_refuses_dirty_staged_missing_and_untracked_overlap() {
    for action in ["dirty", "staged", "missing", "new"] {
        let repo = Repo::new();
        match action {
            "missing" => fs::remove_file(repo.context.component_dir.join("src/main.rs")).unwrap(),
            "new" => repo.write("component/src/new.rs", "untracked"),
            _ => repo.write("component/src/main.rs", "dirty"),
        }
        if action == "staged" {
            repo.git(&["add", "component/src/main.rs"]);
            repo.write("component/src/main.rs", "before\n");
        }
        assert!(
            ensure_clean(&repo.context, &capture(&repo.context).unwrap()).is_err(),
            "{action}"
        );
    }
}

#[test]
fn checkpoint_refuses_head_worktree_and_staged_overlap_drift() {
    for action in ["head", "worktree", "index", "branch"] {
        let repo = Repo::new();
        let pending = repo.pending();
        let head = repo.git(&["rev-parse", "HEAD"]);
        match action {
            "head" => {
                repo.git(&["commit", "--allow-empty", "-qm", "race"]);
            }
            "branch" => {
                repo.git(&["switch", "-qc", "other"]);
            }
            "worktree" => repo.write("component/src/main.rs", "raced\n"),
            _ => {
                repo.git(&["add", "component/src/main.rs"]);
            }
        }
        assert!(finish(&repo.context, &pending).is_err(), "{action}");
        if action != "head" {
            assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
        }
    }
}

#[test]
fn checkpoint_finish_is_idempotent_and_recovers_ref_advance() {
    let repo = Repo::new();
    let pending = repo.pending();
    let restored: PreparedCommit =
        serde_json::from_slice(&serde_json::to_vec(&pending).unwrap()).unwrap();
    repo.git(&[
        "update-ref",
        &pending.branch,
        &pending.commit_id,
        &pending.expected_head,
    ]);
    assert_eq!(finish(&repo.context, &restored).unwrap(), pending.commit_id);
    assert_eq!(finish(&repo.context, &restored).unwrap(), pending.commit_id);
    assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]), "2");
    assert_eq!(repo.git(&["diff", "--cached", "--name-only"]), "");
}

#[test]
fn checkpoint_revalidates_ledger_paths_and_hashes_before_access() {
    let repo = Repo::new();
    let pending = repo.pending();
    for path in [
        "../outside",
        "src/../../outside",
        ".kvist/continuous.json",
        "src",
        "/etc/passwd",
        "tests/.git/config",
    ] {
        let mut bad = pending.clone();
        let state = bad.after.entries.values().next().unwrap().clone();
        bad.after.entries.insert(path.to_owned(), state);
        assert!(finish(&repo.context, &bad).is_err(), "{path}");
    }
    let mut bad = pending.clone();
    bad.commit_id = "--help".to_owned();
    assert!(finish(&repo.context, &bad).is_err());
    let mut bad = pending.clone();
    bad.after.entries.values_mut().next().unwrap().mode = 0o120000;
    assert!(finish(&repo.context, &bad).is_err());
    let mut bad = pending;
    bad.after.entries.values_mut().next().unwrap().digest = "invalid".to_owned();
    assert!(finish(&repo.context, &bad).is_err());
}

#[test]
fn checkpoint_inventory_bounds_reject_oversized_deep_and_conflicting_records() {
    let repo = Repo::new();
    let baseline = capture(&repo.context).unwrap();
    let entry = baseline.entries.values().next().unwrap().clone();
    let mut invalid = baseline.clone();
    invalid.entries.get_mut("src/main.rs").unwrap().size = MAX_SCOPE_FILE_BYTES + 1;
    assert!(invalid.validate(&repo.context).is_err());
    let mut invalid = baseline.clone();
    invalid.entries.insert(
        format!("src/{}file", "directory/".repeat(MAX_SCOPE_DEPTH)),
        entry.clone(),
    );
    assert!(invalid.validate(&repo.context).is_err());
    let mut invalid = baseline.clone();
    for i in 0..MAX_SCOPE_ENTRIES {
        invalid.entries.insert(format!("src/{i}"), entry.clone());
    }
    assert!(invalid.validate(&repo.context).is_err());
    let mut invalid = baseline;
    invalid
        .entries
        .insert("src/main.rs/nested".to_owned(), entry);
    assert!(invalid.validate(&repo.context).is_err());
    fs::write(
        repo.context.component_dir.join("tests/oversized"),
        vec![0; MAX_SCOPE_FILE_BYTES as usize + 1],
    )
    .unwrap();
    assert!(capture(&repo.context).is_err());
}

#[test]
fn checkpoint_filters_and_hooks_are_never_invoked() {
    let repo = Repo::new();
    repo.git(&[
        "config",
        "filter.must-not-run.clean",
        "/nonexistent/kvist-checkpoint-filter",
    ]);
    repo.git(&["config", "filter.must-not-run.required", "true"]);
    repo.write(".gitattributes", "component/src/* filter=must-not-run\n");
    let hooks = repo.directory.path().join("checkpoint-hooks");
    fs::create_dir(&hooks).unwrap();
    fs::write(hooks.join("reference-transaction"), "#!/bin/sh\nexit 97\n").unwrap();
    fs::set_permissions(
        hooks.join("reference-transaction"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    repo.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
    let pending = repo.pending();
    finish(&repo.context, &pending).unwrap();
    ensure_clean(&repo.context, &capture(&repo.context).unwrap()).unwrap();
    assert_eq!(repo.git(&["show", "HEAD:component/src/main.rs"]), "after");
    assert_eq!(
        fs::read_to_string(repo.directory.path().join(".gitattributes")).unwrap(),
        "component/src/* filter=must-not-run\n"
    );
}

#[test]
fn checkpoint_rejects_links_and_excludes_other_kvist_evidence() {
    let repo = Repo::new();
    fs::create_dir_all(repo.context.component_dir.join(".kvist/continuous-reviews")).unwrap();
    repo.write("component/.kvist/continuous.json", "private");
    assert_eq!(capture(&repo.context).unwrap().entries.len(), 2);
    std::os::unix::fs::symlink(
        repo.directory.path().join("unrelated"),
        repo.context.component_dir.join("src/link"),
    )
    .unwrap();
    assert!(capture(&repo.context).is_err());
}

#[test]
fn checkpoint_recovers_only_its_dead_owned_index_lock() {
    let repo = Repo::new();
    let pending = repo.pending();
    let index = index_path(&repo.context).unwrap();
    let mut lock = IndexLock::acquire(&index, &pending.commit_id).unwrap();
    let (path, mut marker) = lock.marker.take().unwrap();
    let metadata = lock.file.metadata().unwrap();
    let owner = LockOwner {
        commit: pending.commit_id.clone(),
        pid: i32::MAX as u32,
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    marker.set_len(0).unwrap();
    marker.seek(SeekFrom::Start(0)).unwrap();
    marker
        .write_all(&serde_json::to_vec(&owner).unwrap())
        .unwrap();
    marker.sync_all().unwrap();
    lock.installed = true;
    drop(lock);
    repo.git(&[
        "update-ref",
        &pending.branch,
        &pending.commit_id,
        &pending.expected_head,
    ]);
    finish(&repo.context, &pending).unwrap();
    assert!(!path.exists());
    assert!(!index.with_extension("lock").exists());
    assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn checkpoint_never_removes_an_unknown_or_live_index_lock() {
    let repo = Repo::new();
    let pending = repo.pending();
    let index = index_path(&repo.context).unwrap();
    let lock_path = PathBuf::from(format!("{}.lock", index.display()));
    fs::write(&lock_path, "unrelated owner").unwrap();
    assert!(finish(&repo.context, &pending).is_err());
    assert_eq!(fs::read(&lock_path).unwrap(), b"unrelated owner");
    fs::remove_file(&lock_path).unwrap();
    let lock = IndexLock::acquire(&index, &pending.commit_id).unwrap();
    assert!(finish(&repo.context, &pending).is_err());
    assert!(lock.owned());
}

#[test]
fn checkpoint_preserves_staging_added_after_prepare_and_supports_linked_worktree() {
    let repo = Repo::new();
    let worktree = repo.directory.path().join("linked");
    repo.git(&[
        "worktree",
        "add",
        "-qb",
        "linked",
        worktree.to_str().unwrap(),
    ]);
    let context = TaskContext {
        project_dir: worktree.clone(),
        component_dir: worktree.join("component"),
        component_path: PathBuf::from("component"),
    };
    let before = capture(&context).unwrap();
    let head = ensure_clean(&context, &before).unwrap();
    fs::write(context.component_dir.join("src/main.rs"), "linked\n").unwrap();
    let after = capture(&context).unwrap();
    let pending = prepare(&context, &head, &before, &after, "linked").unwrap();
    fs::write(worktree.join("unrelated"), "staged\n").unwrap();
    git(&context, None, &["add", "unrelated"], &[], 8192).unwrap();
    finish(&context, &pending).unwrap();
    assert_eq!(
        git_text(&context, None, &["show", ":unrelated"]).unwrap(),
        "staged"
    );
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        git_text(&context, None, &["rev-parse", "HEAD"]).unwrap(),
        pending.commit_id
    );
}

#[test]
fn checkpoint_signing_policy_failure_never_advances_head() {
    let repo = Repo::new();
    let before = capture(&repo.context).unwrap();
    let head = ensure_clean(&repo.context, &before).unwrap();
    repo.git(&["config", "commit.gpgsign", "true"]);
    repo.git(&["config", "gpg.format", "ssh"]);
    assert!(ensure_clean(&repo.context, &before).is_err());
    repo.git(&["config", "gpg.format", "openpgp"]);
    repo.git(&[
        "config",
        "gpg.program",
        "/nonexistent/kvist-checkpoint-signer",
    ]);
    repo.write("component/src/main.rs", "sign me\n");
    let after = capture(&repo.context).unwrap();
    assert!(prepare(&repo.context, &head, &before, &after, "signing").is_err());
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repo.git(&["diff", "--cached", "--name-only"]), "");
}

const PROCESS_FIXTURE: &str = "task_commands::checkpoint::tests::checkpoint_subprocess_fixture";

fn fixture(repo: &Repo, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", PROCESS_FIXTURE, "--nocapture"])
        .env("KVIST_CHECKPOINT_FIXTURE_MODE", mode)
        .env(
            "KVIST_CHECKPOINT_FIXTURE_PID",
            repo.directory.path().join("child.pid"),
        )
        .current_dir(repo.directory.path());
    command
}

fn read_pid(path: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(contents) = fs::read_to_string(path)
            && let Ok(pid) = contents.parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "fixture did not publish its PID");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn reaped(pid: i32) {
    assert_eq!(
        nix::sys::wait::waitpid(
            nix::unistd::Pid::from_raw(pid),
            Some(nix::sys::wait::WaitPidFlag::WNOHANG)
        ),
        Err(nix::errno::Errno::ECHILD),
        "owned direct child was not reaped"
    );
    assert_eq!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
        Err(nix::errno::Errno::ESRCH),
        "owned child survived return"
    );
}

#[test]
fn checkpoint_subprocess_fixture() {
    let Ok(mode) = std::env::var("KVIST_CHECKPOINT_FIXTURE_MODE") else {
        return;
    };
    let pid_path = PathBuf::from(std::env::var_os("KVIST_CHECKPOINT_FIXTURE_PID").unwrap());
    fs::write(&pid_path, std::process::id().to_string()).unwrap();
    if mode == "interrupt-controller" {
        let nested_pid = pid_path.with_extension("nested");
        let mut nested = Command::new(std::env::current_exe().unwrap());
        nested
            .args(["--exact", PROCESS_FIXTURE, "--nocapture"])
            .env("KVIST_CHECKPOINT_FIXTURE_MODE", "stall")
            .env("KVIST_CHECKPOINT_FIXTURE_PID", &nested_pid);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                read_pid(&nested_pid);
                nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGINT)
                    .unwrap();
            });
            let error = process::run(nested, &["fixture"], &[], 8192).unwrap_err();
            assert!(error.to_string().contains("cancel"), "{error}");
        });
        reaped(read_pid(&nested_pid));
        return;
    }
    if matches!(
        mode.as_str(),
        "descendant" | "orphan-stdio" | "silent-descendant"
    ) {
        let mut nested = Command::new(std::env::current_exe().unwrap());
        nested
            .args(["--exact", PROCESS_FIXTURE, "--nocapture"])
            .env(
                "KVIST_CHECKPOINT_FIXTURE_MODE",
                if mode == "silent-descendant" {
                    "closed"
                } else {
                    "stall"
                },
            )
            .env(
                "KVIST_CHECKPOINT_FIXTURE_PID",
                pid_path.with_extension("descendant"),
            );
        #[expect(
            clippy::zombie_processes,
            reason = "The fixture must exit before its helper to exercise orphaned-group cleanup"
        )]
        let mut child = nested.spawn().unwrap();
        if mode == "descendant" {
            let _ = child.wait();
        }
        return;
    }
    if mode.starts_with("transaction") {
        let mut update: Vec<String> = Vec::new();
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let line = line.unwrap();
            fs::write("transaction-phase", &line).unwrap();
            match line.as_str() {
                "start" if mode != "transaction-start" => {
                    println!("start: ok");
                    std::io::stdout().flush().unwrap();
                }
                "prepare" if mode != "transaction-prepare" => {
                    println!("prepare: ok");
                    std::io::stdout().flush().unwrap();
                }
                "commit" if mode == "transaction-advance" => {
                    let output = Command::new("git")
                        .args(["update-ref", "HEAD", &update[0], &update[1]])
                        .output()
                        .unwrap();
                    assert!(output.status.success(), "{output:?}");
                    fs::write("ref-advanced", "yes").unwrap();
                    break;
                }
                "commit" => break,
                _ if line.starts_with("update HEAD ") => {
                    update = line.split(' ').skip(2).map(str::to_owned).collect();
                }
                _ => {}
            }
            if (line == "start" && mode == "transaction-start")
                || (line == "prepare" && mode == "transaction-prepare")
            {
                break;
            }
        }
    }
    if mode == "overflow" {
        std::io::stdout().write_all(&[b'x'; 8192]).unwrap();
    }
    if mode == "stderr-overflow" {
        std::io::stderr().write_all(&[b'x'; 16384]).unwrap();
    }
    if mode == "closed" {
        nix::unistd::close(1).unwrap();
        nix::unistd::close(2).unwrap();
    }
    loop {
        std::thread::park_timeout(Duration::from_secs(60));
    }
}

#[test]
fn checkpoint_process_deadlines_unblock_output_and_full_stdin_and_reap() {
    for (mode, input) in [
        ("stall", Vec::new()),
        ("stall", vec![b'x'; 2 * 1024 * 1024]),
        ("closed", Vec::new()),
    ] {
        let repo = Repo::new();
        let started = Instant::now();
        let error = process::with_limits(
            Duration::from_millis(500),
            sav::CancellationToken::new(),
            || process::run(fixture(&repo, mode), &["fixture"], &input, 8192),
        )
        .unwrap_err();
        assert!(error.to_string().contains("deadline"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(4));
        reaped(read_pid(&repo.directory.path().join("child.pid")));
    }
}

#[test]
fn checkpoint_process_cancellation_and_capture_overflow_reap() {
    for mode in ["stall", "overflow", "stderr-overflow"] {
        let repo = Repo::new();
        let cancellation = sav::CancellationToken::new();
        std::thread::scope(|scope| {
            let token = cancellation.clone();
            let pid_path = repo.directory.path().join("child.pid");
            if mode == "stall" {
                scope.spawn(move || {
                    read_pid(&pid_path);
                    token.cancel();
                });
            }
            let error = process::with_limits(Duration::from_secs(3), cancellation, || {
                process::run(fixture(&repo, mode), &["fixture"], &[], 128)
            })
            .unwrap_err();
            let expected = if mode == "stall" { "cancel" } else { "byte" };
            assert!(error.to_string().contains(expected), "{error}");
        });
        reaped(read_pid(&repo.directory.path().join("child.pid")));
    }
}

#[test]
fn checkpoint_process_precancelled_does_not_spawn() {
    let repo = Repo::new();
    let cancellation = sav::CancellationToken::new();
    cancellation.cancel();
    let error = process::with_limits(Duration::from_secs(3), cancellation, || {
        process::run(fixture(&repo, "stall"), &["fixture"], &[], 8192)
    })
    .unwrap_err();
    assert!(error.to_string().contains("cancel"), "{error}");
    assert!(!repo.directory.path().join("child.pid").exists());
    assert_eq!(process::COMMAND_TIMEOUT, Duration::from_secs(300));
}

fn not_running(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat"));
        if stat
            .as_ref()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            || stat.as_ref().is_ok_and(|s| {
                s.rsplit_once(')')
                    .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
            })
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "owned helper {pid} survived group termination"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn checkpoint_process_kills_group_helpers_but_preserves_unrelated_group() {
    struct Foreign(std::process::Child);
    impl Drop for Foreign {
        fn drop(&mut self) {
            if matches!(self.0.try_wait(), Ok(None)) {
                let _ = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(self.0.id() as i32),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
            let _ = self.0.wait();
        }
    }
    let foreign_repo = Repo::new();
    let mut command = fixture(&foreign_repo, "stall");
    command
        .process_group(0)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut foreign = Foreign(command.spawn().unwrap());
    read_pid(&foreign_repo.directory.path().join("child.pid"));
    for mode in ["descendant", "orphan-stdio", "silent-descendant"] {
        let repo = Repo::new();
        let result = process::with_limits(
            Duration::from_millis(500),
            sav::CancellationToken::new(),
            || process::run(fixture(&repo, mode), &["fixture"], &[], 8192),
        );
        if mode == "silent-descendant" {
            result.unwrap();
        } else {
            let error = result.unwrap_err();
            assert!(error.to_string().contains("deadline"), "{error}");
        }
        reaped(read_pid(&repo.directory.path().join("child.pid")));
        not_running(read_pid(&repo.directory.path().join("child.descendant")));
        assert!(
            foreign.0.try_wait().unwrap().is_none(),
            "unrelated group was terminated"
        );
    }
}

#[test]
fn checkpoint_process_interrupt_cancels_only_its_owned_group() {
    let repo = Repo::new();
    process::with_limits(
        Duration::from_secs(5),
        sav::CancellationToken::new(),
        || {
            process::run(
                fixture(&repo, "interrupt-controller"),
                &["fixture"],
                &[],
                8192,
            )
        },
    )
    .unwrap();
    reaped(read_pid(&repo.directory.path().join("child.pid")));
}

#[test]
fn checkpoint_transaction_deadline_covers_every_blocked_reply_and_ref_retry() {
    for mode in [
        "transaction-start",
        "transaction-prepare",
        "transaction-commit",
        "transaction-advance",
    ] {
        let repo = Repo::new();
        let prepared = repo.pending();
        let index = fs::read(index_path(&repo.context).unwrap()).unwrap();
        let timeout = Duration::from_millis(if mode == "transaction-advance" {
            1500
        } else {
            500
        });
        let error = process::with_limits(timeout, sav::CancellationToken::new(), || {
            process::fixture_transaction(
                fixture(&repo, mode),
                &prepared.commit_id,
                &prepared.expected_head,
                || Ok(()),
            )
        })
        .unwrap_err();
        assert!(error.to_string().contains("deadline"), "{error}");
        reaped(read_pid(&repo.directory.path().join("child.pid")));
        assert_eq!(fs::read(index_path(&repo.context).unwrap()).unwrap(), index);
        if mode == "transaction-advance" {
            assert!(repo.directory.path().join("ref-advanced").exists());
            assert_eq!(repo.git(&["rev-parse", "HEAD"]), prepared.commit_id);
            finish(&repo.context, &prepared).unwrap();
            finish(&repo.context, &prepared).unwrap();
            assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]), "2");
        } else {
            assert_eq!(repo.git(&["rev-parse", "HEAD"]), prepared.expected_head);
        }
    }
}

#[test]
fn checkpoint_transaction_nested_git_inherits_deadline_and_cancellation() {
    for cancel in [false, true] {
        let repo = Repo::new();
        let nested = Repo::new();
        let prepared = repo.pending();
        let cancellation = sav::CancellationToken::new();
        std::thread::scope(|scope| {
            if cancel {
                let pid_path = nested.directory.path().join("child.pid");
                let token = cancellation.clone();
                scope.spawn(move || {
                    read_pid(&pid_path);
                    token.cancel();
                });
            }
            // This case starts two instrumented binaries before the nested wait.
            // Keep startup contention separate from cancellation's readiness gate.
            let timeout = Duration::from_secs(if cancel { 5 } else { 2 });
            let error = process::with_limits(timeout, cancellation, || {
                process::fixture_transaction(
                    fixture(&repo, "transaction-commit"),
                    &prepared.commit_id,
                    &prepared.expected_head,
                    || process::run(fixture(&nested, "stall"), &["fixture"], &[], 8192).map(|_| ()),
                )
            })
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains(if cancel { "cancel" } else { "deadline" }),
                "{error}"
            );
        });
        reaped(read_pid(&repo.directory.path().join("child.pid")));
        reaped(read_pid(&nested.directory.path().join("child.pid")));
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), prepared.expected_head);
    }
}

#[test]
fn checkpoint_transaction_cancellation_unblocks_each_reply() {
    for (mode, phase) in [
        ("transaction-start", "start"),
        ("transaction-prepare", "prepare"),
        ("transaction-commit", "commit"),
    ] {
        let repo = Repo::new();
        let prepared = repo.pending();
        let cancellation = sav::CancellationToken::new();
        std::thread::scope(|scope| {
            let token = cancellation.clone();
            let phase_path = repo.directory.path().join("transaction-phase");
            scope.spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(2);
                while !fs::read_to_string(&phase_path).is_ok_and(|value| value == phase) {
                    assert!(
                        Instant::now() < deadline,
                        "transaction fixture did not reach phase"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                token.cancel();
            });
            let error = process::with_limits(Duration::from_secs(3), cancellation, || {
                process::fixture_transaction(
                    fixture(&repo, mode),
                    &prepared.commit_id,
                    &prepared.expected_head,
                    || Ok(()),
                )
            })
            .unwrap_err();
            assert!(error.to_string().contains("cancel"), "{error}");
        });
        reaped(read_pid(&repo.directory.path().join("child.pid")));
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), prepared.expected_head);
    }
}

#[test]
fn checkpoint_signer_deadline_preserves_head_index_and_retries_without_replay() {
    let repo = Repo::new();
    let before = capture(&repo.context).unwrap();
    let head = ensure_clean(&repo.context, &before).unwrap();
    let index = fs::read(index_path(&repo.context).unwrap()).unwrap();
    let signer = repo.directory.path().join("stalled-signer");
    fs::write(
        &signer,
        "#!/bin/sh\nprintf '%s' \"$$\" > signer.pid\nwhile :; do :; done\n",
    )
    .unwrap();
    fs::set_permissions(&signer, fs::Permissions::from_mode(0o755)).unwrap();
    repo.git(&["config", "gpg.program", signer.to_str().unwrap()]);
    repo.git(&["config", "commit.gpgsign", "true"]);
    repo.write("component/src/main.rs", "completed execution\n");
    let after = capture(&repo.context).unwrap();
    let error = process::with_limits(
        Duration::from_millis(800),
        sav::CancellationToken::new(),
        || prepare(&repo.context, &head, &before, &after, "checkpoint"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("deadline"), "{error}");
    assert!(repo.directory.path().join("signer.pid").exists());
    not_running(read_pid(&repo.directory.path().join("signer.pid")));
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(index_path(&repo.context).unwrap()).unwrap(), index);
    repo.git(&["config", "commit.gpgsign", "false"]);
    let prepared = prepare(&repo.context, &head, &before, &after, "checkpoint").unwrap();
    finish(&repo.context, &prepared).unwrap();
    assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]), "2");
}
