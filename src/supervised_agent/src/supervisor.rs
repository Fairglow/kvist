use std::{
    io::{self, Read, Write},
    os::{fd::AsFd, unix::process::CommandExt},
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use nix::{
    errno::Errno,
    fcntl::{FcntlArg, OFlag, fcntl},
    poll::{PollFd, PollFlags, poll},
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use signal_hook::{
    consts::{SIGINT, SIGTERM},
    iterator::{Handle as SignalHandle, Signals},
};

use crate::{Error, Result};

const MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(3_600);
const MAX_RETRIES: u32 = 10;
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const LOOP_BUFFER_BYTES: usize = 4_096;
const STREAM_CHANNEL_CAPACITY: usize = 16;
const STREAM_CHUNK_BYTES: usize = 4_096;
const STREAM_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const STREAM_POLL_TIMEOUT_MILLISECONDS: u16 = 50;
const OUTPUT_READY_TIMEOUT_MILLISECONDS: u16 = 1_000;

/// Hard-bounded controls applied to each supervised host process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupervisionPolicy {
    /// Maximum duration without bytes from either output stream.
    pub idle_timeout: Duration,
    /// Whether stdout is inspected for deterministic repetition.
    pub detect_loops: bool,
    /// Number of retries after the initial attempt.
    pub max_retries: u32,
    /// Combined stdout and stderr bytes accepted across one attempt.
    pub max_output_bytes: usize,
}

impl SupervisionPolicy {
    fn validate(&self) -> Result<()> {
        if self.idle_timeout.is_zero() || self.idle_timeout > MAX_IDLE_TIMEOUT {
            return Err(Error::InvalidPolicy {
                reason: "idle timeout must be between 1 and 3600 seconds".to_owned(),
            });
        }
        if self.max_retries > MAX_RETRIES {
            return Err(Error::InvalidPolicy {
                reason: format!("automatic retries must be at most {MAX_RETRIES}"),
            });
        }
        if self.max_output_bytes == 0 || self.max_output_bytes > MAX_OUTPUT_BYTES {
            return Err(Error::InvalidPolicy {
                reason: format!("output limit must be between 1 and {MAX_OUTPUT_BYTES} bytes"),
            });
        }
        Ok(())
    }
}

/// Failure classes for which the current supervisor permits a retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryCause {
    /// Neither output stream produced bytes before the idle deadline.
    IdleTimeout,
    /// The stdout suffix contained a supported repetition pattern.
    RepetitionLoop,
}

impl RetryCause {
    fn description(self) -> &'static str {
        match self {
            Self::IdleTimeout => "an idle timeout",
            Self::RepetitionLoop => "a repetition loop",
        }
    }
}

/// Inputs supplied to a caller before it constructs one attempt command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptContext {
    /// One-based attempt number.
    pub attempt_number: u32,
    /// Retryable failure from the previous attempt, if this is a retry.
    pub prior_failure: Option<RetryCause>,
}

impl AttemptContext {
    /// Returns advisory prompt text describing prior side-effect uncertainty.
    pub fn retry_notice(&self) -> Option<String> {
        self.prior_failure.map(|failure| {
            format!(
                "[Supervisor retry context]\n\
                 This is attempt {} after the prior attempt ended because of {}. \
                 The prior attempt may have changed files or external systems. \
                 Inspect and reconcile current state before repeating any non-idempotent action.",
                self.attempt_number,
                failure.description()
            )
        })
    }
}

/// Program, arguments, and working directory for one host-process attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Program passed directly to `exec`, without a shell.
    pub program: String,
    /// Exact arguments passed to the program.
    pub arguments: Vec<String>,
    /// Optional process working directory.
    pub working_directory: Option<PathBuf>,
}

impl CommandSpec {
    /// Creates a command using the caller's current working directory.
    pub fn new<I, S>(program: impl Into<String>, arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: program.into(),
            arguments: arguments.into_iter().map(Into::into).collect(),
            working_directory: None,
        }
    }

    /// Selects the working directory for the attempt.
    pub fn in_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(directory.into());
        self
    }
}

/// Summary returned after a command exits successfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionReport {
    /// Total attempts including the successful attempt.
    pub attempts: u32,
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

enum StreamEvent {
    Data(Stream, Vec<u8>),
    ReadFailed(Stream, io::Error),
    OutputLimitExceeded,
}

enum AttemptEvent {
    Exited(ExitStatus),
    Retry(RetryCause),
}

struct SignalCancellation {
    requested: Arc<AtomicBool>,
    handle: SignalHandle,
    listener: Option<JoinHandle<()>>,
}

impl SignalCancellation {
    fn new() -> Result<Self> {
        let mut signals = Signals::new([SIGINT, SIGTERM]).map_err(|source| Error::Io {
            operation: "install supervision signal handlers",
            path: PathBuf::from("process signals"),
            source,
        })?;
        let handle = signals.handle();
        let requested = Arc::new(AtomicBool::new(false));
        let listener_flag = Arc::clone(&requested);
        let listener = std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                listener_flag.store(true, Ordering::Release);
            }
        });
        Ok(Self {
            requested,
            handle,
            listener: Some(listener),
        })
    }

    fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

impl Drop for SignalCancellation {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}

/// Runs commands produced by `command_for_attempt` under the supplied policy.
///
/// This is host-process supervision, not a filesystem, credential, or network
/// isolation boundary.
pub fn run_supervised<F>(
    policy: &SupervisionPolicy,
    mut command_for_attempt: F,
) -> Result<ExecutionReport>
where
    F: FnMut(&AttemptContext) -> Result<CommandSpec>,
{
    policy.validate()?;
    let cancellation = SignalCancellation::new()?;
    let mut prior_failure = None;

    for attempt_number in 1..=policy.max_retries + 1 {
        if cancellation.requested() {
            return Err(Error::Cancelled);
        }
        let context = AttemptContext {
            attempt_number,
            prior_failure,
        };
        let specification = command_for_attempt(&context)?;
        if specification.program.trim().is_empty() {
            return Err(Error::InvalidCommandTemplate {
                reason: "program must not be empty".to_owned(),
            });
        }

        let event = run_attempt(&specification, policy, &cancellation)?;
        match event {
            AttemptEvent::Exited(status) if status.success() => {
                return Ok(ExecutionReport {
                    attempts: attempt_number,
                });
            }
            AttemptEvent::Exited(status) => return Err(Error::ProcessFailed { status }),
            AttemptEvent::Retry(cause) if attempt_number <= policy.max_retries => {
                prior_failure = Some(cause);
                eprintln!(
                    "[supervisor] retrying after {} (attempt {}/{})",
                    cause.description(),
                    attempt_number + 1,
                    policy.max_retries + 1
                );
            }
            AttemptEvent::Retry(cause) => {
                return Err(Error::SupervisionExhausted {
                    attempts: attempt_number,
                    reason: format!(
                        "maximum automatic retries exhausted after {}",
                        cause.description()
                    ),
                });
            }
        }
    }

    unreachable!("validated retry bound always executes at least one attempt")
}

fn run_attempt(
    specification: &CommandSpec,
    policy: &SupervisionPolicy,
    cancellation: &SignalCancellation,
) -> Result<AttemptEvent> {
    let mut command = Command::new(&specification.program);
    command
        .args(&specification.arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if let Some(directory) = &specification.working_directory {
        command.current_dir(directory);
    }

    let mut child = command.spawn().map_err(|source| Error::Io {
        operation: "spawn supervised command",
        path: PathBuf::from(&specification.program),
        source,
    })?;
    let stdout = child.stdout.take().ok_or_else(|| Error::Io {
        operation: "capture supervised stdout",
        path: PathBuf::from("stdout"),
        source: io::Error::other("child stdout was not piped"),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| Error::Io {
        operation: "capture supervised stderr",
        path: PathBuf::from("stderr"),
        source: io::Error::other("child stderr was not piped"),
    })?;

    let (sender, receiver) = mpsc::sync_channel(STREAM_CHANNEL_CAPACITY);
    let budget = Arc::new(OutputBudget::new(policy.max_output_bytes));
    let stop_readers = Arc::new(AtomicBool::new(false));
    let stdout_reader = spawn_reader(
        stdout,
        Stream::Stdout,
        sender.clone(),
        Arc::clone(&budget),
        Arc::clone(&stop_readers),
    );
    let stderr_reader = spawn_reader(
        stderr,
        Stream::Stderr,
        sender,
        budget,
        Arc::clone(&stop_readers),
    );
    let event = monitor(&mut child, &receiver, policy, cancellation);

    let termination = terminate_process_group(&mut child, &specification.program);
    let draining = drain_to_end(&receiver, policy.max_output_bytes, &stop_readers);
    let stdout_join = join_reader(stdout_reader, "read supervised stdout");
    let stderr_join = join_reader(stderr_reader, "read supervised stderr");

    termination?;
    draining?;
    stdout_join?;
    stderr_join?;
    event
}

fn spawn_reader(
    mut reader: impl Read + AsFd + Send + 'static,
    stream: Stream,
    sender: SyncSender<StreamEvent>,
    budget: Arc<OutputBudget>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let flags = match fcntl(&reader, FcntlArg::F_GETFL) {
            Ok(flags) => OFlag::from_bits_truncate(flags),
            Err(error) => {
                let _ = send_stream_event(
                    &sender,
                    &stop,
                    StreamEvent::ReadFailed(stream, io::Error::from_raw_os_error(error as i32)),
                );
                return;
            }
        };
        if let Err(error) = fcntl(&reader, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK)) {
            let _ = send_stream_event(
                &sender,
                &stop,
                StreamEvent::ReadFailed(stream, io::Error::from_raw_os_error(error as i32)),
            );
            return;
        }

        let mut buffer = [0_u8; STREAM_CHUNK_BYTES];
        while !stop.load(Ordering::Acquire) {
            let mut descriptors = [PollFd::new(
                reader.as_fd(),
                PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR,
            )];
            match poll(&mut descriptors, STREAM_POLL_TIMEOUT_MILLISECONDS) {
                Ok(0) | Err(Errno::EINTR) => continue,
                Err(error) => {
                    let _ = send_stream_event(
                        &sender,
                        &stop,
                        StreamEvent::ReadFailed(stream, io::Error::from_raw_os_error(error as i32)),
                    );
                    return;
                }
                Ok(_) => {}
            }
            match reader.read(&mut buffer) {
                Ok(0) => return,
                Ok(count) => {
                    let accepted = budget.reserve(count);
                    if accepted > 0
                        && !send_stream_event(
                            &sender,
                            &stop,
                            StreamEvent::Data(stream, buffer[..accepted].to_vec()),
                        )
                    {
                        return;
                    }
                    if accepted < count {
                        if !budget.reported.swap(true, Ordering::AcqRel) {
                            let _ =
                                send_stream_event(&sender, &stop, StreamEvent::OutputLimitExceeded);
                        }
                        return;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => {
                    let _ =
                        send_stream_event(&sender, &stop, StreamEvent::ReadFailed(stream, error));
                    return;
                }
            }
        }
    })
}

fn send_stream_event(
    sender: &SyncSender<StreamEvent>,
    stop: &AtomicBool,
    mut event: StreamEvent,
) -> bool {
    loop {
        match sender.try_send(event) {
            Ok(()) => return true,
            Err(mpsc::TrySendError::Full(returned)) if !stop.load(Ordering::Acquire) => {
                event = returned;
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(mpsc::TrySendError::Full(_)) | Err(mpsc::TrySendError::Disconnected(_)) => {
                return false;
            }
        }
    }
}

fn monitor(
    child: &mut Child,
    receiver: &Receiver<StreamEvent>,
    policy: &SupervisionPolicy,
    cancellation: &SignalCancellation,
) -> Result<AttemptEvent> {
    let mut last_output = Instant::now();
    let mut loop_buffer = String::new();

    loop {
        if cancellation.requested() {
            return Err(Error::Cancelled);
        }
        if let Some(status) = child.try_wait().map_err(|source| Error::Io {
            operation: "inspect supervised command status",
            path: PathBuf::from("process"),
            source,
        })? {
            return Ok(AttemptEvent::Exited(status));
        }

        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(StreamEvent::Data(stream, bytes)) => {
                last_output = Instant::now();
                forward(&stream, &bytes)?;
                if policy.detect_loops && matches!(stream, Stream::Stdout) {
                    append_loop_text(&mut loop_buffer, &bytes);
                    if contains_repetition(&loop_buffer) {
                        return Ok(AttemptEvent::Retry(RetryCause::RepetitionLoop));
                    }
                }
            }
            Ok(StreamEvent::ReadFailed(stream, source)) => {
                return Err(Error::Io {
                    operation: match stream {
                        Stream::Stdout => "read supervised stdout",
                        Stream::Stderr => "read supervised stderr",
                    },
                    path: match stream {
                        Stream::Stdout => PathBuf::from("stdout"),
                        Stream::Stderr => PathBuf::from("stderr"),
                    },
                    source,
                });
            }
            Ok(StreamEvent::OutputLimitExceeded) => {
                return Err(Error::OutputLimitExceeded {
                    max_bytes: policy.max_output_bytes,
                });
            }
            Err(RecvTimeoutError::Timeout) => {
                if last_output.elapsed() >= policy.idle_timeout {
                    return Ok(AttemptEvent::Retry(RetryCause::IdleTimeout));
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if last_output.elapsed() >= policy.idle_timeout {
                    return Ok(AttemptEvent::Retry(RetryCause::IdleTimeout));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn drain_to_end(
    receiver: &Receiver<StreamEvent>,
    max_output_bytes: usize,
    stop_readers: &AtomicBool,
) -> Result<()> {
    let mut failure = None;
    let deadline = Instant::now() + STREAM_DRAIN_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            stop_readers.store(true, Ordering::Release);
            while let Ok(event) = receiver.recv_timeout(Duration::from_millis(100)) {
                record_stream_event(event, max_output_bytes, &mut failure);
            }
            return failure.map_or(Err(Error::OutputStreamsRetained), Err);
        }
        match receiver.recv_timeout(remaining) {
            Ok(event) => record_stream_event(event, max_output_bytes, &mut failure),
            Err(RecvTimeoutError::Disconnected) => return failure.map_or(Ok(()), Err),
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn record_stream_event(event: StreamEvent, max_output_bytes: usize, failure: &mut Option<Error>) {
    if failure.is_some() {
        return;
    }
    *failure = match event {
        StreamEvent::Data(stream, bytes) => forward(&stream, &bytes).err(),
        StreamEvent::ReadFailed(stream, source) => Some(Error::Io {
            operation: match stream {
                Stream::Stdout => "read supervised stdout",
                Stream::Stderr => "read supervised stderr",
            },
            path: match stream {
                Stream::Stdout => PathBuf::from("stdout"),
                Stream::Stderr => PathBuf::from("stderr"),
            },
            source,
        }),
        StreamEvent::OutputLimitExceeded => Some(Error::OutputLimitExceeded {
            max_bytes: max_output_bytes,
        }),
    };
}

fn forward(stream: &Stream, bytes: &[u8]) -> Result<()> {
    match stream {
        Stream::Stdout => write_when_ready(io::stdout().lock(), bytes, "stdout"),
        Stream::Stderr => write_when_ready(io::stderr().lock(), bytes, "stderr"),
    }
}

fn write_when_ready(
    mut destination: impl Write + AsFd,
    bytes: &[u8],
    name: &'static str,
) -> Result<()> {
    let mut descriptors = [PollFd::new(destination.as_fd(), PollFlags::POLLOUT)];
    let ready =
        poll(&mut descriptors, OUTPUT_READY_TIMEOUT_MILLISECONDS).map_err(|error| Error::Io {
            operation: "poll supervised output",
            path: PathBuf::from(name),
            source: io::Error::from_raw_os_error(error as i32),
        })?;
    if ready == 0 {
        return Err(Error::Io {
            operation: "forward supervised output",
            path: PathBuf::from(name),
            source: io::Error::new(
                io::ErrorKind::TimedOut,
                "output destination did not become writable",
            ),
        });
    }
    destination.write_all(bytes).map_err(|source| Error::Io {
        operation: "forward supervised output",
        path: PathBuf::from(name),
        source,
    })?;
    destination.flush().map_err(|source| Error::Io {
        operation: "flush supervised output",
        path: PathBuf::from(name),
        source,
    })
}

struct OutputBudget {
    limit: usize,
    used: AtomicUsize,
    reported: AtomicBool,
}

impl OutputBudget {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            used: AtomicUsize::new(0),
            reported: AtomicBool::new(false),
        }
    }

    fn reserve(&self, requested: usize) -> usize {
        loop {
            let used = self.used.load(Ordering::Acquire);
            let accepted = requested.min(self.limit.saturating_sub(used));
            if self
                .used
                .compare_exchange(used, used + accepted, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return accepted;
            }
        }
    }
}

fn terminate_process_group(child: &mut Child, program: &str) -> Result<()> {
    let pid = i32::try_from(child.id()).map_err(|_| Error::Io {
        operation: "identify supervised process group",
        path: PathBuf::from(program),
        source: io::Error::other("child process identifier exceeds Linux pid range"),
    })?;
    match killpg(Pid::from_raw(pid), Signal::SIGKILL) {
        Ok(()) | Err(Errno::ESRCH) => {}
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Io {
                operation: "terminate supervised process group",
                path: PathBuf::from(program),
                source: io::Error::from_raw_os_error(error as i32),
            });
        }
    }
    child.wait().map_err(|source| Error::Io {
        operation: "wait for terminated process group",
        path: PathBuf::from(program),
        source,
    })?;
    Ok(())
}

fn join_reader(handle: JoinHandle<()>, operation: &'static str) -> Result<()> {
    handle.join().map_err(|_| Error::Io {
        operation,
        path: PathBuf::from("process stream"),
        source: io::Error::other("output reader thread panicked"),
    })
}

fn append_loop_text(buffer: &mut String, bytes: &[u8]) {
    buffer.push_str(&String::from_utf8_lossy(bytes));
    if buffer.len() > LOOP_BUFFER_BYTES {
        let mut start = buffer.len() - LOOP_BUFFER_BYTES;
        while !buffer.is_char_boundary(start) {
            start += 1;
        }
        buffer.drain(..start);
    }
}

fn contains_repetition(buffer: &str) -> bool {
    let bytes = buffer.as_bytes();
    for length in 10..=512 {
        if bytes.len() >= length * 3 {
            let end = bytes.len();
            let first = &bytes[end - length * 3..end - length * 2];
            let second = &bytes[end - length * 2..end - length];
            let third = &bytes[end - length..];
            if first == second && second == third {
                return true;
            }
        }
    }

    let lines = buffer
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.len() >= 4 {
        let end = lines.len();
        if lines[end - 4..].windows(2).all(|pair| pair[0] == pair[1]) {
            return true;
        }
    }
    if lines.len() >= 6 {
        let end = lines.len();
        let suffix = &lines[end - 6..];
        if suffix[0] == suffix[2]
            && suffix[2] == suffix[4]
            && suffix[1] == suffix[3]
            && suffix[3] == suffix[5]
        {
            return true;
        }
    }
    false
}
