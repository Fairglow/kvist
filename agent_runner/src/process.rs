//! Private, single-threaded tool-process supervision.

use std::io::{self, Read, Write};
use std::os::fd::AsFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use agent_runtime::CancellationToken;
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::sys::signal::{Signal, killpg};
use nix::sys::wait::{Id, WaitPidFlag, WaitStatus, waitid};
use nix::unistd::Pid;

use crate::error::{Error, Result};
use crate::sandbox::ToolOutcome;

const CHUNK_BYTES: usize = 8192;
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const DRAIN_GRACE: Duration = Duration::from_millis(250);
const REAP_GRACE: Duration = Duration::from_secs(1);

pub(crate) fn run(
    command: &mut Command,
    input: Option<&[u8]>,
    wall_time: Duration,
    output_limit: usize,
    cancellation: &CancellationToken,
    spawn_error: impl FnOnce(io::Error) -> Error,
) -> Result<ToolOutcome> {
    crate::executor::check_cancelled(cancellation)?;
    let started = Instant::now();
    let child = command
        .process_group(0)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(spawn_error)?;
    let mut owned = OwnedChild::new(child)?;
    let result = pump(
        &mut owned,
        input.unwrap_or_default(),
        started,
        wall_time,
        output_limit,
        cancellation,
    );
    let cleanup = owned.cleanup();
    match (result, cleanup) {
        (Ok(mut pumped), Ok(status)) => {
            pumped.outcome.exited = true;
            pumped.outcome.status = status.code();
            if pumped.incomplete_input && !pumped.outcome.failed() {
                return Err(failure(
                    "child exited successfully before the complete request was written",
                ));
            }
            Ok(pumped.outcome)
        }
        (Err(error), Ok(_)) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(failure(format!("{error}; {cleanup}"))),
    }
}

struct Pumped {
    outcome: ToolOutcome,
    incomplete_input: bool,
}

fn pump(
    owned: &mut OwnedChild,
    input: &[u8],
    started: Instant,
    wall_time: Duration,
    limit: usize,
    cancellation: &CancellationToken,
) -> Result<Pumped> {
    let mut stdout = Some(
        owned
            .child
            .stdout
            .take()
            .ok_or_else(|| failure("child did not provide standard output"))?,
    );
    let mut stderr = Some(
        owned
            .child
            .stderr
            .take()
            .ok_or_else(|| failure("child did not provide standard error"))?,
    );
    let mut stdin = owned.child.stdin.take();
    if !input.is_empty() && stdin.is_none() {
        return Err(failure("child did not provide standard input"));
    }
    if let Some(pipe) = &stdout {
        nonblocking(pipe)?;
    }
    if let Some(pipe) = &stderr {
        nonblocking(pipe)?;
    }
    if let Some(pipe) = &stdin {
        nonblocking(pipe)?;
    }
    if input.is_empty() {
        stdin = None;
    }

    let mut outcome = ToolOutcome::rejected();
    let mut written: usize = 0;
    let mut exit_seen = None;
    let mut stopping = None;
    loop {
        if stopping.is_none() {
            outcome.cancelled = cancellation.is_cancelled();
            outcome.timed_out = !outcome.cancelled && started.elapsed() >= wall_time;
            if outcome.cancelled || outcome.timed_out || outcome.output_limit_exceeded {
                owned.terminate()?;
                stdin = None;
                stopping = Some(Instant::now());
            }
        }

        let mut remaining = limit
            .saturating_sub(outcome.stdout.len())
            .saturating_sub(outcome.stderr.len());
        let mut progress = drain(
            &mut stdout,
            &mut outcome.stdout,
            &mut remaining,
            &mut outcome.output_limit_exceeded,
            "standard output",
        )?;
        progress |= drain(
            &mut stderr,
            &mut outcome.stderr,
            &mut remaining,
            &mut outcome.output_limit_exceeded,
            "standard error",
        )?;

        if owned.exited()? {
            exit_seen.get_or_insert_with(Instant::now);
            stdin = None;
        }
        if exit_seen.is_some() && stdout.is_none() && stderr.is_none() {
            break;
        }
        if stopping.is_some_and(|when| when.elapsed() >= DRAIN_GRACE)
            || exit_seen.is_some_and(|when| when.elapsed() >= DRAIN_GRACE)
        {
            return Err(failure(
                "process cleanup/output pipes did not complete within the drain window; \
                 a descendant may retain a pipe or have escaped the owned process group",
            ));
        }

        // Output is checked before sending more input, including overflow on this turn.
        if stopping.is_none()
            && !outcome.output_limit_exceeded
            && let Some(pipe) = &mut stdin
        {
            let end = input.len().min(written.saturating_add(CHUNK_BYTES));
            match pipe.write(&input[written..end]) {
                Ok(0) => return Err(failure("write child request: zero-length write")),
                Ok(count) => {
                    written += count;
                    progress = true;
                    if written == input.len() {
                        stdin = None;
                    }
                }
                // A failed tool may reject the request before consuming stdin.
                // Its exit status and partial diagnostics remain a ToolOutcome.
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => stdin = None,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(failure(format!("write child request: {error}"))),
            }
        }
        if !progress {
            thread::sleep(POLL_INTERVAL);
        }
    }
    Ok(Pumped {
        outcome,
        incomplete_input: written != input.len(),
    })
}

fn nonblocking(pipe: &impl AsFd) -> Result<()> {
    let flags = fcntl(pipe, FcntlArg::F_GETFL)
        .map(OFlag::from_bits_retain)
        .map_err(|error| failure(format!("inspect child pipe flags: {error}")))?;
    fcntl(pipe, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))
        .map_err(|error| failure(format!("set child pipe nonblocking: {error}")))?;
    Ok(())
}

fn drain<R: Read>(
    pipe: &mut Option<R>,
    capture: &mut Vec<u8>,
    remaining: &mut usize,
    overflow: &mut bool,
    name: &str,
) -> Result<bool> {
    let Some(reader) = pipe else {
        return Ok(false);
    };
    let mut chunk = [0; CHUNK_BYTES];
    match reader.read(&mut chunk) {
        Ok(0) => {
            *pipe = None;
            Ok(true)
        }
        Ok(count) => {
            let retained = count.min(*remaining);
            capture.extend_from_slice(&chunk[..retained]);
            *remaining -= retained;
            *overflow |= retained < count;
            Ok(true)
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(failure(format!("read child {name}: {error}"))),
    }
}

struct OwnedChild {
    child: Child,
    pid: Pid,
    termination_attempted: bool,
    cleanup_attempted: bool,
}

impl OwnedChild {
    fn new(mut child: Child) -> Result<Self> {
        let pid = match i32::try_from(child.id()) {
            Ok(pid) if pid > 0 => Pid::from_raw(pid),
            _ => {
                let killed = child.kill();
                let reaped = reap(&mut child);
                return Err(failure(format!(
                    "child PID is not a positive signal target; kill: {killed:?}; reap: {reaped:?}"
                )));
            }
        };
        Ok(Self {
            child,
            pid,
            termination_attempted: false,
            cleanup_attempted: false,
        })
    }

    fn exited(&self) -> Result<bool> {
        // Keep the leader unreaped until after signalling its own group. Its PID
        // cannot be recycled into another process/group during this interval.
        match waitid(
            Id::Pid(self.pid),
            WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
        ) {
            Ok(WaitStatus::StillAlive) | Err(Errno::EINTR) => Ok(false),
            Ok(WaitStatus::Exited(..) | WaitStatus::Signaled(..)) => Ok(true),
            Ok(_) => Err(failure("unexpected child wait status")),
            Err(error) => Err(failure(format!("observe child exit: {error}"))),
        }
    }

    fn terminate(&mut self) -> Result<()> {
        if self.termination_attempted {
            return Ok(());
        }
        self.termination_attempted = true;
        let group = match killpg(self.pid, Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => Ok(()),
            Err(error) => Err(error),
        };
        // The unreaped direct child may have joined a different group.
        let direct = self.child.kill().or_else(|error| {
            if error.raw_os_error() == Some(Errno::ESRCH as i32) {
                Ok(())
            } else {
                Err(error)
            }
        });
        match (group, direct) {
            (Ok(()), Ok(())) => Ok(()),
            (group, direct) => Err(failure(format!(
                "kill owned process group: {group:?}; direct child kill: {direct:?}"
            ))),
        }
    }

    fn cleanup(&mut self) -> Result<ExitStatus> {
        self.cleanup_attempted = true;
        let termination = self.terminate();
        let reaped = reap(&mut self.child);
        match (termination, reaped) {
            (Ok(()), Ok(status)) => Ok(status),
            (Err(error), Ok(_)) | (Ok(()), Err(error)) => Err(error),
            (Err(error), Err(reap)) => Err(failure(format!("{error}; {reap}"))),
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.cleanup_attempted {
            let _ = self.cleanup();
        }
    }
}

fn reap(child: &mut Child) -> Result<ExitStatus> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(failure(format!("reap child: {error}"))),
        }
        if started.elapsed() >= REAP_GRACE {
            return Err(failure(
                "child did not reap within the cleanup window after SIGKILL; \
                 uninterruptible kernel work cannot be guaranteed terminated",
            ));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn failure(reason: impl Into<String>) -> Error {
    Error::SandboxExec {
        tool: None,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    struct ReadError(io::ErrorKind);

    impl Read for ReadError {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }
    }

    #[test]
    fn eof_closes_pipe_but_io_failure_is_not_eof() {
        let mut eof = Some(io::empty());
        let mut capture = Vec::new();
        let mut remaining = 10;
        let mut overflow = false;
        assert!(
            drain(
                &mut eof,
                &mut capture,
                &mut remaining,
                &mut overflow,
                "test"
            )
            .unwrap()
        );
        assert!(eof.is_none());
        let mut broken = Some(ReadError(io::ErrorKind::PermissionDenied));
        let error = drain(
            &mut broken,
            &mut capture,
            &mut remaining,
            &mut overflow,
            "test",
        )
        .unwrap_err();
        assert!(error.to_string().contains("read child test"));
        assert!(broken.is_some());
        assert!(!overflow);
    }

    #[test]
    fn interrupted_and_would_block_reads_keep_pipe_open() {
        for kind in [io::ErrorKind::Interrupted, io::ErrorKind::WouldBlock] {
            let mut pipe = Some(ReadError(kind));
            let mut capture = Vec::new();
            let mut remaining = 10;
            let mut overflow = false;
            assert!(
                !drain(
                    &mut pipe,
                    &mut capture,
                    &mut remaining,
                    &mut overflow,
                    "test"
                )
                .unwrap()
            );
            assert!(pipe.is_some());
            assert_eq!(remaining, 10);
            assert!(capture.is_empty());
        }
    }

    #[test]
    fn every_drain_shares_remaining_capacity_including_partial_chunks() {
        let mut out = Some(io::Cursor::new(b"12345678"));
        let mut err = Some(io::Cursor::new(b"abcdefgh"));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut remaining = 10;
        let mut overflow = false;
        drain(&mut out, &mut stdout, &mut remaining, &mut overflow, "out").unwrap();
        drain(&mut err, &mut stderr, &mut remaining, &mut overflow, "err").unwrap();
        assert_eq!(stdout, b"12345678");
        assert_eq!(stderr, b"ab");
        assert_eq!(remaining, 0);
        assert!(overflow);
    }

    #[test]
    fn setup_failure_still_kills_and_reaps_owned_process() {
        let child = Command::new("/usr/bin/sleep")
            .arg("1.5")
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut owned = OwnedChild::new(child).unwrap();
        let error = match pump(
            &mut owned,
            &[],
            Instant::now(),
            Duration::from_secs(1),
            100,
            &CancellationToken::new(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("setup unexpectedly succeeded"),
        };
        assert!(error.to_string().contains("standard output"));
        let status = owned.cleanup().unwrap();
        assert_eq!(status.signal(), Some(Signal::SIGKILL as i32));
        assert!(owned.child.try_wait().unwrap().is_some());
    }

    #[test]
    fn private_host_supervision_honors_short_timeout_and_zero_capture_cap() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf partial; exec sleep 1.5"]);
        let outcome = run(
            &mut command,
            None,
            Duration::from_millis(100),
            100,
            &CancellationToken::new(),
            |error| failure(format!("spawn fixture: {error}")),
        )
        .unwrap();
        assert!(outcome.timed_out);
        assert!(outcome.exited);
        assert_eq!(outcome.stdout, b"partial");
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf x"]);
        let outcome = run(
            &mut command,
            None,
            Duration::from_secs(1),
            0,
            &CancellationToken::new(),
            |error| failure(format!("spawn fixture: {error}")),
        )
        .unwrap();
        assert!(outcome.output_limit_exceeded);
        assert!(outcome.stdout.is_empty());
        assert!(outcome.stderr.is_empty());
    }
}
