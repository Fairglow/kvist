//! Real executor-path regressions using finite, local subprocess fixtures.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use galla::protocol::SandboxRequest;
use sav::{CancellationToken, ToolIntent};
use serde_json::json;
use skott::sandbox::{BuildRequest, build_request, default_resources, execute};
use skott::{HostExecutor, SandboxPaths, ToolExecutor, ToolOutcome, ToolPolicy, ToolRegistry};
use tempfile::{Builder, TempDir};

fn fixture(script: &str, limit: u64) -> (TempDir, SandboxPaths, SandboxRequest) {
    let dir = Builder::new()
        .prefix(".subprocess-test-")
        .tempdir_in(".")
        .unwrap();
    let root = dir.path().canonicalize().unwrap();
    let work = root.join("work");
    std::fs::create_dir(&work).unwrap();
    let runner = root.join("runner");
    std::fs::write(root.join("script"), format!("{script}\n")).unwrap();
    // Never rewrite an executable while concurrent spawns may inherit its write
    // handle. Per-test commands are ordinary input files for an immutable launcher.
    std::fs::hard_link(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/subprocess_runner.sh"),
        &runner,
    )
    .unwrap();
    let paths = SandboxPaths {
        runner,
        backend: PathBuf::from("/usr/bin/true"),
    };
    let policy = ToolPolicy::minimum();
    let argv = vec!["/usr/bin/true".into()];
    let request = build_request(
        &paths,
        &BuildRequest {
            argv: &argv,
            working_directory: &work,
            read_roots: &[],
            environment: BTreeMap::new(),
            policy: &policy,
            resources: galla::protocol::Resources {
                max_output_bytes: limit,
                wall_time_ms: 3000,
                ..default_resources()
            },
        },
    )
    .unwrap();
    (dir, paths, request)
}

fn host(script: &str, token: &CancellationToken) -> skott::Result<ToolOutcome> {
    HostExecutor::new(
        ToolRegistry::new(ToolPolicy::minimum()),
        std::env::current_dir().unwrap(),
    )
    .execute(
        &ToolIntent {
            id: "supervision".into(),
            provider_id: None,
            name: "shell".into(),
            arguments: json!({"command":script}),
        },
        token,
    )
}

fn assert_bounded_overflow(outcome: &ToolOutcome, limit: usize) {
    assert!(
        outcome.output_limit_exceeded,
        "stdout={}, stderr={}",
        outcome.stdout.len(),
        outcome.stderr.len()
    );
    assert_eq!(outcome.stdout.len() + outcome.stderr.len(), limit);
    assert!(outcome.failed());
}

#[test]
fn sandbox_combined_capture_is_exactly_bounded_at_rapid_exit() {
    let (_dir, paths, request) = fixture(
        "cat >/dev/null; head -c 700 /dev/zero; head -c 700 /dev/zero >&2",
        1000,
    );
    let result = execute(&paths, &request, &CancellationToken::new()).unwrap();
    assert_bounded_overflow(&result, 1000);
}

#[test]
fn sandbox_final_drain_cannot_bypass_capture_limit() {
    let (_dir, paths, request) = fixture("cat >/dev/null; head -c 200000 /dev/zero", 1000);
    let result = execute(&paths, &request, &CancellationToken::new()).unwrap();
    assert_bounded_overflow(&result, 1000);
}

#[test]
fn host_combined_capture_is_exactly_bounded_at_rapid_exit() {
    let result = host(
        "head -c 6000 /dev/zero; head -c 6000 /dev/zero >&2",
        &CancellationToken::new(),
    )
    .unwrap();
    assert_bounded_overflow(&result, 8192);
}

#[test]
fn host_final_drain_cannot_bypass_capture_limit() {
    let result = host("head -c 200000 /dev/zero", &CancellationToken::new()).unwrap();
    assert_bounded_overflow(&result, 8192);
}

#[test]
fn sandbox_retained_pipes_are_explicit_failure_not_delayed_success() {
    let (_dir, paths, request) = fixture("cat >/dev/null; sleep 1.5 & exit 0", 1000);
    let started = Instant::now();
    let result = execute(&paths, &request, &CancellationToken::new());
    assert!(result.is_err(), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(result.unwrap_err().to_string().contains("pipe"));
}

#[test]
fn host_retained_pipes_are_explicit_failure_not_delayed_success() {
    let started = Instant::now();
    let result = host("sleep 1.5 & exit 0", &CancellationToken::new());
    assert!(result.is_err(), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(result.unwrap_err().to_string().contains("pipe"));
}

#[test]
fn ordinary_nonzero_exit_keeps_status_and_invalid_utf8_partial_diagnostics() {
    let script = "printf '\\377ok'; printf 'failure' >&2; exit 7";
    let (_dir, paths, request) = fixture(&format!("cat >/dev/null; {script}"), 1000);
    for outcome in [
        host(script, &CancellationToken::new()).unwrap(),
        execute(&paths, &request, &CancellationToken::new()).unwrap(),
    ] {
        assert!(outcome.exited);
        assert_eq!(outcome.status, Some(7));
        assert_eq!(outcome.stdout, b"\xffok");
        assert_eq!(outcome.stderr, b"failure");
        assert_eq!(outcome.output_text(100), "\u{fffd}ok");
        assert!(outcome.failed());
    }
}

#[test]
fn ordinary_success_and_exact_limit_are_not_overflow() {
    let (_dir, paths, request) = fixture("cat >/dev/null; printf '12345'; printf '67890' >&2", 10);
    let outcome = execute(&paths, &request, &CancellationToken::new()).unwrap();
    assert_eq!(outcome.stdout, b"12345");
    assert_eq!(outcome.stderr, b"67890");
    assert_eq!(outcome.status, Some(0));
    assert!(!outcome.failed());
    let outcome = host("printf 'ok'", &CancellationToken::new()).unwrap();
    assert_eq!(outcome.stdout, b"ok");
    assert!(!outcome.failed());
}

#[test]
fn precancel_preserves_error_and_never_spawns() {
    let (_dir, paths, request) = fixture("exit 0", 1000);
    let token = CancellationToken::new();
    token.cancel();
    for error in [
        execute(&paths, &request, &token).unwrap_err(),
        host("exit 0", &token).unwrap_err(),
    ] {
        assert!(error.to_string().contains("cancelled before"));
    }
}

#[test]
fn sandbox_blocked_stdin_obeys_wall_time_and_preserves_timeout_outcome() {
    let (_dir, paths, mut request) = fixture("printf 'started'; exec sleep 1.5", 1000);
    request
        .environment
        .insert("PAYLOAD".into(), "x".repeat(200_000));
    request.resources.wall_time_ms = 100;
    let started = Instant::now();
    let result = execute(&paths, &request, &CancellationToken::new());
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "blocked stdin exceeded the bounded deadline"
    );
    let outcome = result.unwrap();
    assert!(outcome.timed_out);
    assert!(outcome.exited);
    assert!(outcome.failed());
    assert_eq!(outcome.stdout, b"started");
}

#[test]
fn sandbox_blocked_stdin_obeys_cancellation_and_preserves_partial_output() {
    let (_dir, paths, mut request) = fixture("printf 'started'; exec sleep 1.5", 1000);
    request
        .environment
        .insert("PAYLOAD".into(), "x".repeat(200_000));
    let token = CancellationToken::new();
    let canceller = token.clone();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        canceller.cancel();
    });
    let started = Instant::now();
    let result = execute(&paths, &request, &token);
    worker.join().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "blocked stdin ignored cancellation"
    );
    let outcome = result.unwrap();
    assert!(outcome.cancelled);
    assert!(outcome.exited);
    assert_eq!(outcome.stdout, b"started");
}

#[test]
fn host_cancellation_kills_owned_group_and_preserves_partial_output() {
    let token = CancellationToken::new();
    let canceller = token.clone();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        canceller.cancel();
    });
    let started = Instant::now();
    let result = host("printf 'started'; sleep 1.5", &token);
    worker.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    let outcome = result.unwrap();
    assert!(outcome.cancelled);
    assert!(outcome.exited);
    assert_eq!(outcome.stdout, b"started");
    assert!(outcome.failed());
}

#[test]
fn sandbox_early_nonzero_exit_during_request_write_retains_tool_outcome() {
    let (_dir, paths, mut request) = fixture("printf 'rejected' >&2; exit 9", 1000);
    request
        .environment
        .insert("PAYLOAD".into(), "x".repeat(200_000));
    let outcome = execute(&paths, &request, &CancellationToken::new()).unwrap();
    assert!(outcome.exited);
    assert_eq!(outcome.status, Some(9));
    assert_eq!(outcome.stderr, b"rejected");
    assert!(outcome.failed());
}

#[test]
fn sandbox_writes_complete_large_request_without_deadlocking_on_output() {
    let (_dir, paths, mut request) =
        fixture("head -c 200000 /dev/zero; cat; printf 'done' >&2", 1 << 20);
    request
        .environment
        .insert("PAYLOAD".into(), "x".repeat(200_000));
    let expected = serde_json::to_vec(&request).unwrap();
    let outcome = execute(&paths, &request, &CancellationToken::new()).unwrap();
    assert!(!outcome.failed());
    assert_eq!(&outcome.stdout[..200_000], vec![0; 200_000]);
    assert_eq!(&outcome.stdout[200_000..], expected);
    assert_eq!(outcome.stderr, b"done");
}

struct RecordedProcess(PathBuf);

impl Drop for RecordedProcess {
    fn drop(&mut self) {
        if let Ok(text) = std::fs::read_to_string(&self.0)
            && let Ok(pid) = text.trim().parse::<i32>()
            && pid > 0
        {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

#[test]
fn host_escaped_pipe_holder_fails_boundedly_and_is_explicitly_cleaned_by_fixture() {
    let dir = Builder::new()
        .prefix(".escaped-pipe-test-")
        .tempdir_in(".")
        .unwrap();
    let pid_file = dir.path().canonicalize().unwrap().join("pid");
    let _cleanup = RecordedProcess(pid_file.clone());
    // The escaped process is finite even if a failed assertion bypasses cleanup.
    let script = format!(
        "setsid /bin/sh -c 'echo $$ > {}; exec sleep 1.5' & \
         while [ ! -s {} ]; do sleep 0.01; done; exit 0",
        pid_file.display(),
        pid_file.display()
    );
    let started = Instant::now();
    let result = host(&script, &CancellationToken::new());
    assert!(result.is_err(), "escaped descriptor was treated as success");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(result.unwrap_err().to_string().contains("pipe"));
}

#[test]
fn sandbox_output_overflow_kills_command_ignoring_sigterm() {
    let (_dir, paths, request) = fixture(
        "cat >/dev/null; trap '' TERM; head -c 10000 /dev/zero; exec sleep 1.5",
        1000,
    );
    let started = Instant::now();
    let outcome = execute(&paths, &request, &CancellationToken::new()).unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_bounded_overflow(&outcome, 1000);
    assert!(outcome.exited);
}

#[test]
fn independent_calls_do_not_share_cancellation_or_process_groups() {
    let token = CancellationToken::new();
    let first_token = token.clone();
    let first =
        std::thread::spawn(move || host("printf 'first'; exec sleep 1.5", &first_token).unwrap());
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        token.cancel();
    });
    let second = host("sleep 0.2; printf 'second'", &CancellationToken::new()).unwrap();
    canceller.join().unwrap();
    let first = first.join().unwrap();
    assert!(first.cancelled);
    assert_eq!(first.stdout, b"first");
    assert!(!second.failed());
    assert_eq!(second.stdout, b"second");
}

#[test]
fn native_helper_near_7000_byte_output_is_usable_through_host_executor() {
    let (dir, _paths, _request) = fixture("exit 0", 1000);
    let work = dir.path().canonicalize().unwrap().join("work");
    std::fs::write(work.join("large"), "x".repeat(16_384)).unwrap();
    let executor = HostExecutor::new(
        ToolRegistry::new(ToolPolicy::minimum())
            .with_file_helper(env!("CARGO_BIN_EXE_skott-file-tool")),
        work,
    );
    let outcome = executor
        .execute(
            &ToolIntent {
                id: "native-output-bound".into(),
                provider_id: None,
                name: "read_file".into(),
                arguments: json!({"path":"/workspace/large","limit":16384}),
            },
            &CancellationToken::new(),
        )
        .unwrap();
    assert!(!outcome.failed(), "{}", outcome.error_text(1000));
    assert!((6500..=7001).contains(&outcome.stdout.len()));
    let decoded: serde_json::Value = serde_json::from_slice(&outcome.stdout).unwrap();
    assert!(decoded["content"].as_str().unwrap().len() > 6000);
}

#[test]
fn successful_early_stdin_close_is_not_false_success() {
    let (_dir, paths, mut request) = fixture("exit 0", 1000);
    request
        .environment
        .insert("PAYLOAD".into(), "x".repeat(200_000));
    let error = execute(&paths, &request, &CancellationToken::new()).unwrap_err();
    assert!(error.to_string().contains("complete request"));
}

#[test]
fn missing_installed_runner_remains_a_fail_closed_spawn_error() {
    let (dir, mut paths, request) = fixture("exit 0", 1000);
    paths.runner = dir.path().join("missing-runner");
    let error = execute(&paths, &request, &CancellationToken::new()).unwrap_err();
    assert!(matches!(error, skott::Error::SandboxUnavailable { .. }));
}

struct OwnedFixtureCleanup(PathBuf);

impl OwnedFixtureCleanup {
    fn pid(&self) -> nix::unistd::Pid {
        let pid = std::fs::read_to_string(&self.0)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        assert!(pid > 0);
        nix::unistd::Pid::from_raw(pid)
    }
}

impl Drop for OwnedFixtureCleanup {
    fn drop(&mut self) {
        use nix::sys::signal::{Signal, kill, killpg};
        use nix::sys::wait::{Id, WaitPidFlag, WaitStatus, waitid, waitpid};
        let Ok(text) = std::fs::read_to_string(&self.0) else {
            return;
        };
        let Ok(pid) = text.trim().parse::<i32>() else {
            return;
        };
        if pid <= 0 {
            return;
        }
        let pid = nix::unistd::Pid::from_raw(pid);
        // Never signal a recycled PID: only a still-owned, unreaped child pins it.
        if waitid(
            Id::Pid(pid),
            WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
        )
        .is_err()
        {
            return;
        }
        let _ = killpg(pid, Signal::SIGKILL);
        let _ = kill(pid, Signal::SIGKILL);
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(1) {
            match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::StillAlive) => std::thread::sleep(Duration::from_millis(5)),
                _ => break,
            }
        }
    }
}

fn moved_direct_child(trigger: &str, retained_member: bool) {
    use nix::errno::Errno;
    use nix::sys::wait::{Id, WaitPidFlag, waitid};
    use nix::unistd::{getpgid, getpgrp};

    let (dir, paths, mut request) = fixture("exit 0", 1000);
    let root = dir.path().canonicalize().unwrap();
    let program = root.join("move_group.py");
    let pid_file = root.join("direct-pid");
    let member_file = root.join("member-pid");
    let cleanup = OwnedFixtureCleanup(pid_file.clone());
    std::fs::write(
        &program,
        r#"import os, select, sys, time
from pathlib import Path
root = Path(sys.argv[1])
pid = os.getpid()
(root / "direct-pid.new").write_text(str(pid))
os.replace(root / "direct-pid.new", root / "direct-pid")
assert os.getpgrp() == pid
if sys.argv[2] == "member":
    read_end, write_end = os.pipe()
    member = os.fork()
    if member == 0:
        os.close(read_end)
        os.write(write_end, b"ready")
        os.close(write_end)
        time.sleep(5)
        os._exit(0)
    (root / "member-pid").write_text(str(member))
    os.close(write_end)
    assert select.select([read_end], [], [], 1)[0], "member readiness timed out"
    assert os.read(read_end, 5) == b"ready"
    os.close(read_end)
    assert os.getpgid(member) == pid
os.setpgid(0, os.getpgid(os.getppid()))
(root / "ready").write_text("ready")
os.write(1, b"ready")
if sys.argv[3] == "overflow":
    os.write(1, b"x" * 20000)
time.sleep(5)
"#,
    )
    .unwrap();
    let command = format!(
        "exec /usr/bin/python3 {} {} {} {trigger}",
        program.display(),
        root.display(),
        if retained_member { "member" } else { "empty" }
    );
    std::fs::write(root.join("script"), format!("cat >/dev/null\n{command}\n")).unwrap();
    let supervisor_group = getpgrp();
    let token = CancellationToken::new();
    let canceller = if trigger == "cancellation" {
        let ready_file = root.join("ready");
        let token = token.clone();
        Some(std::thread::spawn(move || {
            let started = Instant::now();
            while !std::fs::metadata(&ready_file).is_ok_and(|metadata| metadata.len() > 0)
                && started.elapsed() < Duration::from_secs(1)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            token.cancel();
        }))
    } else {
        None
    };
    request.resources.wall_time_ms = 500;
    let started = Instant::now();
    let result = if trigger == "cancellation" {
        host(&command, &token)
    } else {
        execute(&paths, &request, &token)
    };
    if let Some(canceller) = canceller {
        canceller.join().unwrap();
    }
    assert_eq!(getpgrp(), supervisor_group);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(
        pid_file.exists(),
        "fixture failed before PID publication: {}",
        match &result {
            Ok(outcome) => format!(
                "status={:?}, timeout={}, overflow={}, stderr={}",
                outcome.status,
                outcome.timed_out,
                outcome.output_limit_exceeded,
                outcome.error_text(1000)
            ),
            Err(error) => error.to_string(),
        }
    );
    let pid = cleanup.pid();
    assert_eq!(
        waitid(
            Id::Pid(pid),
            WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
        ),
        Err(Errno::ECHILD),
        "direct child must be terminated and reaped before executor return"
    );
    if retained_member {
        let member = std::fs::read_to_string(member_file)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        assert!(
            getpgid(Some(nix::unistd::Pid::from_raw(member))).is_err()
                || std::fs::read_to_string(format!("/proc/{member}/stat"))
                    .is_ok_and(|stat| stat.split_whitespace().nth(2) == Some("Z")),
            "original-group member must also stop"
        );
    }
    let outcome = result.unwrap();
    assert!(outcome.exited);
    assert!(outcome.failed());
    assert!(outcome.stdout.starts_with(b"ready"));
    match trigger {
        "cancellation" => assert!(outcome.cancelled),
        "timeout" => assert!(outcome.timed_out),
        "overflow" => assert_bounded_overflow(&outcome, 1000),
        _ => panic!("unknown fixture trigger"),
    }
}

#[test]
fn moved_direct_child_cancellation_with_empty_original_group() {
    moved_direct_child("cancellation", false);
}

#[test]
fn moved_direct_child_cancellation_with_retained_original_group_member() {
    moved_direct_child("cancellation", true);
}

#[test]
fn moved_direct_child_timeout_with_empty_original_group() {
    moved_direct_child("timeout", false);
}

#[test]
fn moved_direct_child_timeout_with_retained_original_group_member() {
    moved_direct_child("timeout", true);
}

#[test]
fn moved_direct_child_overflow_with_empty_original_group() {
    moved_direct_child("overflow", false);
}

#[test]
fn moved_direct_child_overflow_with_retained_original_group_member() {
    moved_direct_child("overflow", true);
}
