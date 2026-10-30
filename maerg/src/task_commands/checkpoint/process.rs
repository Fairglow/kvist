//! Deadline-bound, nonblocking Git supervision. No worker threads own pipe I/O.

use std::{
    cell::RefCell,
    io::{Read, Write},
    os::{fd::AsFd, unix::process::CommandExt},
    path::Path,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use nix::{
    errno::Errno,
    fcntl::{FcntlArg, OFlag, fcntl},
    poll::{PollFd, PollFlags, PollTimeout, poll},
    sys::{
        signal::{Signal, killpg},
        wait::{Id, WaitPidFlag, WaitStatus, waitid},
    },
    unistd::Pid,
};
use sav::CancellationToken;

use super::{PreparedCommit, TaskContext, fail, head_branch, io, reject};
use crate::Result;

pub(super) const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);
const STDERR_LIMIT: usize = 8192;
const POLL_INTERVAL: u16 = 25;

#[derive(Clone)]
struct Control {
    deadline: Instant,
    cancellation: CancellationToken,
}

thread_local! {
    static ACTIVE: RefCell<Option<Control>> = const { RefCell::new(None) };
}

impl Control {
    fn current() -> Result<Self> {
        let deadline = Instant::now()
            .checked_add(COMMAND_TIMEOUT)
            .ok_or_else(|| fail("cannot establish the five-minute Git command deadline"))?;
        Ok(ACTIVE.with(|active| match &*active.borrow() {
            Some(parent) => Self {
                deadline: deadline.min(parent.deadline),
                cancellation: parent.cancellation.clone(),
            },
            None => Self {
                deadline,
                cancellation: CancellationToken::new(),
            },
        }))
    }

    fn check(&self) -> Result<()> {
        if sav::take_interrupted() {
            self.cancellation.cancel();
        }
        let reason = if self.cancellation.is_cancelled() {
            Some("Git command cancelled")
        } else if Instant::now() >= self.deadline {
            Some("Git command deadline expired (five-minute maximum)")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(fail(format!(
                "{reason}; ref mutation may be unknown. Inspect the retained checkpoint and Git locks, then retry the checkpoint without replaying completed tasks"
            )));
        }
        Ok(())
    }
}

struct Active(Option<Control>);

impl Active {
    fn enter(control: Control) -> Self {
        Self(ACTIVE.with(|active| active.replace(Some(control))))
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.replace(self.0.take());
        });
    }
}

#[cfg(test)]
pub(super) fn with_limits<T>(
    timeout: Duration,
    cancellation: CancellationToken,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| fail("invalid test deadline"))?;
    let _active = Active::enter(Control {
        deadline,
        cancellation,
    });
    action()
}

pub(super) fn git(
    context: &TaskContext,
    index: Option<&Path>,
    args: &[&str],
    input: &[u8],
    limit: u64,
) -> Result<Vec<u8>> {
    let mut command = command(context, index);
    command.args(args);
    run(command, args, input, limit)
}

fn command(context: &TaskContext, index: Option<&Path>) -> Command {
    let mut command = Command::new("git");
    command
        .args([
            "--no-pager",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.splitIndex=false",
        ])
        .current_dir(&context.project_dir)
        .env("LC_ALL", "C")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_PREFIX",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_NAMESPACE",
        "GIT_SHALLOW_FILE",
        "GIT_CEILING_DIRECTORIES",
        "GIT_CONFIG_PARAMETERS",
    ] {
        command.env_remove(variable);
    }
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    command
}

struct Process {
    child: Option<Child>,
    group: Pid,
    input: Option<ChildStdin>,
    output: Option<ChildStdout>,
    errors: Option<ChildStderr>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    consumed: usize,
    limit: usize,
    control: Control,
}

impl Process {
    fn start(mut command: Command, limit: u64) -> Result<Self> {
        reject(
            limit > super::MAX_GIT_OUTPUT,
            "Git capture bound exceeds the 16-MiB maximum",
        )?;
        sav::install_handler();
        let control = Control::current()?;
        control.check()?;
        let limit = usize::try_from(limit).map_err(|_| fail("invalid Git capture bound"))?;
        command
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command
            .spawn()
            .map_err(|e| fail(format!("cannot start Git: {e}; install Git/check PATH")))?;
        // process_group(0) establishes PGID == child PID before exec succeeds.
        // Do not reap this leader until group cleanup; its PID pins group ownership.
        let group = Pid::from_raw(child.id() as i32);
        let mut process = Self {
            child: Some(child),
            group,
            input: None,
            output: None,
            errors: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            consumed: 0,
            limit,
            control,
        };
        if let Some(child) = &mut process.child {
            process.input = child.stdin.take();
            process.output = child.stdout.take();
            process.errors = child.stderr.take();
        }
        nonblocking(process.input.as_ref())?;
        nonblocking(process.output.as_ref())?;
        nonblocking(process.errors.as_ref())?;
        Ok(process)
    }

    fn write(&mut self, mut bytes: &[u8]) -> Result<()> {
        while !bytes.is_empty() {
            self.control.check()?;
            let input = self
                .input
                .as_mut()
                .ok_or_else(|| fail("Git stdin is closed"))?;
            match input.write(bytes) {
                Ok(0) => {
                    return Err(fail(
                        "Git closed stdin before the checkpoint request was written",
                    ));
                }
                Ok(count) => bytes = &bytes[count..],
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => self.tick(true)?,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(fail(format!("cannot write Git stdin: {error}"))),
            }
        }
        Ok(())
    }

    fn reply(&mut self, expected: &[u8]) -> Result<()> {
        loop {
            self.control.check()?;
            let pending = &self.stdout[self.consumed..];
            if let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                reject(
                    &pending[..=end] != expected,
                    "Git ref transaction did not confirm the requested phase; inspect the retained checkpoint",
                )?;
                self.consumed += end + 1;
                return Ok(());
            }
            reject(
                pending.len() >= 128 || self.output.is_none(),
                "Git ref transaction ended or returned an invalid response; ref outcome may be unknown",
            )?;
            self.tick(false)?;
        }
    }

    fn wait(&mut self) -> Result<()> {
        loop {
            self.control.check()?;
            // WNOWAIT observes exit without releasing the PID/PGID ownership pin.
            let exited = match waitid(
                Id::Pid(self.group),
                WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
            ) {
                Ok(WaitStatus::StillAlive) => false,
                Ok(_) => true,
                Err(Errno::EINTR) => continue,
                Err(error) => {
                    return Err(fail(format!("cannot inspect owned Git process: {error}")));
                }
            };
            if exited && self.output.is_none() && self.errors.is_none() {
                return Ok(());
            }
            self.tick(false)?;
        }
    }

    fn tick(&mut self, writing: bool) -> Result<()> {
        self.control.check()?;
        let progress = read_pipe(&mut self.output, &mut self.stdout, self.limit, "stdout")?
            | read_pipe(&mut self.errors, &mut self.stderr, STDERR_LIMIT, "stderr")?;
        if progress {
            return Ok(());
        }
        let mut descriptors = Vec::new();
        if let Some(output) = &self.output {
            descriptors.push(PollFd::new(output.as_fd(), PollFlags::POLLIN));
        }
        if let Some(errors) = &self.errors {
            descriptors.push(PollFd::new(errors.as_fd(), PollFlags::POLLIN));
        }
        if writing && let Some(input) = &self.input {
            descriptors.push(PollFd::new(input.as_fd(), PollFlags::POLLOUT));
        }
        let remaining = self
            .control
            .deadline
            .saturating_duration_since(Instant::now());
        let milliseconds = remaining.as_millis().clamp(1, u128::from(POLL_INTERVAL)) as u16;
        match poll(&mut descriptors, PollTimeout::from(milliseconds)) {
            Ok(_) | Err(Errno::EINTR) => self.control.check(),
            Err(error) => Err(fail(format!("cannot poll owned Git pipes: {error}"))),
        }
    }

    fn reap(&mut self) -> Result<ExitStatus> {
        self.input.take();
        self.output.take();
        self.errors.take();
        reject(self.child.is_none(), "Git child already reaped")?;
        loop {
            match waitid(
                Id::Pid(self.group),
                WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
            ) {
                Ok(_) => break,
                Err(Errno::EINTR) => continue,
                Err(Errno::ECHILD) => {
                    self.child.take();
                    return Err(fail(
                        "Git child ownership was lost; refusing to signal an unverified process group",
                    ));
                }
                Err(error) => {
                    return Err(fail(format!(
                        "cannot verify owned Git process group: {error}"
                    )));
                }
            }
        }
        // Signal only this isolated, still-pinned group, including helpers retaining
        // pipes after the Git leader exits. No process-name or global child waits.
        let killed = match killpg(self.group, Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => Ok(()),
            Err(error) => Err(fail(format!(
                "cannot terminate owned Git process group: {error}"
            ))),
        };
        let waited = self
            .child
            .as_mut()
            .ok_or_else(|| fail("Git child unavailable for reaping"))
            .and_then(|child| io(child.wait()));
        if waited.is_ok() {
            self.child.take();
        }
        match (killed, waited) {
            (Ok(()), status) => status,
            (Err(kill), Ok(_)) => Err(kill),
            (Err(kill), Err(wait)) => Err(fail(format!(
                "{kill}; additionally failed to reap Git: {wait}"
            ))),
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self.child.is_some()
            && let Err(error) = self.reap()
        {
            tracing::error!(%error, "checkpoint Git process cleanup failed");
        }
    }
}

fn nonblocking(pipe: Option<&impl AsFd>) -> Result<()> {
    let pipe = pipe.ok_or_else(|| fail("Git process pipe unavailable"))?;
    let flags = fcntl(pipe, FcntlArg::F_GETFL)
        .map_err(|e| fail(format!("cannot inspect Git pipe: {e}")))?;
    fcntl(
        pipe,
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(|e| fail(format!("cannot make Git pipe nonblocking: {e}")))?;
    Ok(())
}

fn read_pipe<T: Read>(
    pipe: &mut Option<T>,
    bytes: &mut Vec<u8>,
    limit: usize,
    stream: &str,
) -> Result<bool> {
    let Some(reader) = pipe else {
        return Ok(false);
    };
    let mut chunk = [0; 8192];
    match reader.read(&mut chunk) {
        Ok(0) => {
            pipe.take();
            Ok(true)
        }
        Ok(count) => {
            reject(
                count > limit.saturating_sub(bytes.len()),
                &format!(
                    "Git {stream} byte capture limit exceeded; inspect the retained checkpoint"
                ),
            )?;
            bytes.extend_from_slice(&chunk[..count]);
            Ok(true)
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(fail(format!("cannot read Git {stream}: {error}"))),
    }
}

fn supervise<T>(
    command: Command,
    limit: u64,
    action: impl FnOnce(&mut Process) -> Result<T>,
) -> Result<T> {
    let mut process = Process::start(command, limit)?;
    let _active = Active::enter(process.control.clone());
    let result = action(&mut process);
    let status = process.reap();
    match (result, status) {
        (Err(primary), Err(cleanup)) => {
            Err(fail(format!("{primary}; cleanup also failed: {cleanup}")))
        }
        (Err(primary), Ok(_)) if !process.stderr.is_empty() => Err(fail(format!(
            "{primary}; Git stderr: {}",
            String::from_utf8_lossy(&process.stderr)
        ))),
        (Err(primary), Ok(_)) => Err(primary),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Ok(value), Ok(status)) if status.success() => Ok(value),
        (Ok(_), Ok(_)) => Err(fail(format!(
            "Git exited unsuccessfully: {}; inspect signing/Git configuration and retry the retained checkpoint",
            String::from_utf8_lossy(&process.stderr)
        ))),
    }
}

pub(super) fn run(command: Command, _args: &[&str], input: &[u8], limit: u64) -> Result<Vec<u8>> {
    supervise(command, limit, |process| {
        process.write(input)?;
        process.input.take();
        process.wait()?;
        Ok(std::mem::take(&mut process.stdout))
    })
}

pub(super) fn advance(context: &TaskContext, prepared: &PreparedCommit) -> Result<()> {
    let mut command = command(context, None);
    command.args([
        "update-ref",
        "-m",
        "Kvist continuous checkpoint (UNACCEPTED)",
        "--stdin",
    ]);
    transaction(
        command,
        &prepared.commit_id,
        &prepared.expected_head,
        || {
            // update HEAD has locked its symbolic ref and referent. Validate the
            // branch while those locks and the inherited transaction deadline hold.
            reject(
                head_branch(context)? != (prepared.expected_head.clone(), prepared.branch.clone()),
                "attached branch changed before Git locked HEAD",
            )
        },
    )
}

fn transaction(
    command: Command,
    commit: &str,
    expected: &str,
    verify: impl FnOnce() -> Result<()>,
) -> Result<()> {
    supervise(command, 8192, |process| {
        exchange(process, commit, expected, verify)
    })
}

fn exchange(
    process: &mut Process,
    commit: &str,
    expected: &str,
    verify: impl FnOnce() -> Result<()>,
) -> Result<()> {
    process.write(format!("start\nupdate HEAD {commit} {expected}\nprepare\n").as_bytes())?;
    process.reply(b"start: ok\n")?;
    process.reply(b"prepare: ok\n")?;
    verify()?;
    process.write(b"commit\n")?;
    process.reply(b"commit: ok\n")?;
    process.input.take();
    process.wait()
}

#[cfg(test)]
pub(super) fn fixture_transaction(
    command: Command,
    commit: &str,
    expected: &str,
    verify: impl FnOnce() -> Result<()>,
) -> Result<()> {
    supervise(command, 8192, |process| {
        process.reply(b"\n")?;
        process.reply(b"running 1 test\n")?;
        exchange(process, commit, expected, verify)
    })
}
