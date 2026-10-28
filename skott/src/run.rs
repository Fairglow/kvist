//! The worker thread that runs a session loop off the UI thread.
//!
//! The model transport and sandbox executor are blocking, so the loop runs on a
//! owned thread and pushes [`Event`]s over a bounded channel. The UI owns the
//! channel receiver and renders each event. Cancellation flows from a shared
//! [`CancellationToken`], and new prompts flow from the UI over a prompt channel.
//! Steering messages flow over a separate steering channel and are injected at
//! safe interruption points during an active task.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::sync::{Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sav::CancellationToken;
use sav::ModelTransport;

use crate::context::ContextManager;
use crate::error::{Error, Result};
use crate::retry::RetryPolicy;
use crate::session::{
    AgentSession, ControlMailbox, Event, EventSink, Recorder, RunLimits, Skott, Submission,
    ToolExecutor,
};

/// Bounded submission capability. Classification belongs to the run owner,
/// never to potentially stale UI state.
#[derive(Clone)]
pub struct PromptSender {
    controls: ControlMailbox,
    lifetime: Arc<()>,
}

impl PromptSender {
    pub fn send(&self, text: String) -> Result<Submission> {
        self.controls.submit(text).map_err(Into::into)
    }

    /// Requires active work, so a simultaneous completion cannot enqueue a new
    /// task on a model the frontend has already deselected.
    pub fn steer(&self, text: String) -> Result<Submission> {
        self.controls
            .steer(text)
            .map(|id| Submission {
                id,
                kind: crate::SubmissionKind::Guidance,
            })
            .map_err(Into::into)
    }
}

/// Receives [`Event`]s from the worker; implements [`EventSink`].
struct ChannelSink {
    sender: mpsc::SyncSender<Event>,
    shutdown: Arc<AtomicBool>,
}

impl EventSink for ChannelSink {
    fn send(&self, event: Event) -> Result<()> {
        let mut pending = event;
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                return Err(Error::ChannelClosed);
            }
            match self.sender.try_send(pending) {
                Ok(()) => return Ok(()),
                Err(mpsc::TrySendError::Disconnected(_)) => return Err(Error::ChannelClosed),
                Err(mpsc::TrySendError::Full(event)) => pending = event,
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// A handle to a running session, holding the cancellation token and thread.
pub struct SessionHandle {
    cancellation: CancellationToken,
    thread: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
    controls: ControlMailbox,
    recording: bool,
    control_server: Arc<Mutex<Option<sav::observation::ControlServer>>>,
}

impl SessionHandle {
    /// Requests cancellation of the running turn.
    pub fn cancel(&self) {
        if let Err(error) = self.controls.cancel_active(&self.cancellation) {
            self.cancellation.cancel();
            tracing::error!(%error, "active cancellation state unavailable");
        }
    }

    /// Stops the session permanently, including queued prompts.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.controls.shutdown();
        self.cancellation.cancel();
        if let Ok(mut server) = self.control_server.lock() {
            server.take();
        }
    }

    /// Clones the private in-process guidance capability for library callers.
    pub fn controls(&self) -> ControlMailbox {
        self.controls.clone()
    }

    /// Enables a private socket owned by this worker. The embedding host must
    /// additionally require sandboxed execution and agent-inaccessible logging,
    /// and must disclose the returned capability only to the human operator.
    pub fn enable_controls(&self) -> Result<std::path::PathBuf> {
        if !self.recording || self.shutdown.load(Ordering::SeqCst) || !self.is_running() {
            return Err(Error::Control {
                reason: "controls require a live worker with durable recording".into(),
            });
        }
        let mut server = self.control_server.lock().map_err(|_| Error::Control {
            reason: "control endpoint ownership is unavailable".into(),
        })?;
        if self.controls.is_shutdown() {
            return Err(Error::Control {
                reason: "worker is shut down".into(),
            });
        }
        if server.is_none() {
            *server = Some(sav::observation::bind_runtime_controls(
                &self.controls,
                &self.cancellation,
            )?);
        }
        server
            .as_ref()
            .map(|server| server.path().to_owned())
            .ok_or_else(|| Error::Control {
                reason: "control endpoint was not created".into(),
            })
    }

    /// Reports whether the owned worker is alive (including idle prompt waits).
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("session worker failed");
        }
    }
}

/// Starts a session loop on a worker thread and returns a handle, the event
/// receiver owned by the UI, and the prompt sender the UI uses to submit prompts.
///
/// `context` bounds the model's context across the whole session, and an
/// optional `recorder` captures the durable record (journal + transcript,
/// including reasoning) for later inspection.
///
/// `max_turns` caps how many model turns one submitted prompt may drive before
/// the loop stops. A value of one yields a single-shot prompt (the model takes
/// one turn, its tools run, and control returns); a larger value permits the
/// autonomous multi-turn loop that a caller opts into explicitly.
#[allow(clippy::too_many_arguments)]
pub fn start<M, E>(
    session: AgentSession,
    transport: M,
    executor: E,
    context: ContextManager,
    recorder: Option<Box<dyn Recorder>>,
    retry: RetryPolicy,
    max_turns: impl Into<Option<u32>>,
    limits: RunLimits,
) -> Result<(SessionHandle, mpsc::Receiver<Event>, PromptSender)>
where
    M: ModelTransport + Send + 'static,
    E: ToolExecutor + Send + 'static,
{
    start_with_controls(
        session,
        transport,
        executor,
        context,
        recorder,
        retry,
        max_turns.into(),
        limits,
        ControlMailbox::default(),
    )
}

/// Starts a worker with an explicitly supplied private control capability.
#[allow(clippy::too_many_arguments)]
pub fn start_with_controls<M, E>(
    mut session: AgentSession,
    transport: M,
    executor: E,
    mut context: ContextManager,
    mut recorder: Option<Box<dyn Recorder>>,
    retry: RetryPolicy,
    max_turns: impl Into<Option<u32>>,
    limits: RunLimits,
    controls: ControlMailbox,
) -> Result<(SessionHandle, mpsc::Receiver<Event>, PromptSender)>
where
    M: ModelTransport + Send + 'static,
    E: ToolExecutor + Send + 'static,
{
    let max_turns = max_turns.into();
    let prompt_tx = PromptSender {
        controls: controls.clone(),
        lifetime: Arc::new(()),
    };
    let prompt_lifetime = Arc::downgrade(&prompt_tx.lifetime);
    let worker_controls = controls.clone();
    let recording = recorder.is_some();
    let (tx, rx) = mpsc::sync_channel(128);
    let cancellation = CancellationToken::new();
    let handle_cancellation = cancellation.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker_shutdown = Arc::clone(&shutdown);
    let control_server = Arc::new(Mutex::new(None::<sav::observation::ControlServer>));
    let worker_server = Arc::clone(&control_server);
    let thread = thread::Builder::new()
        .name("agent-session".into())
        .spawn(move || {
            let sink = ChannelSink {
                sender: tx,
                shutdown: Arc::clone(&worker_shutdown),
            };
            // The worker stays alive until shutdown or prompt disconnection.
            // `Skott::run` records each prompt's session
            // boundary in the durable recorder, so the worker does not manage the
            // record lifecycle itself. Cancellation re-arms the token after each
            // turn so a cancelled turn does not end the session.
            while let Some(text) =
                wait_for_prompt(&worker_controls, &prompt_lifetime, &worker_shutdown)
            {
                if worker_shutdown.load(Ordering::SeqCst) {
                    break;
                }
                // Label the durable record with the prompt before any turn so
                // the transcript is findable by its content, then record it.
                if let Some(recorder) = recorder.as_mut()
                    && let Err(error) = recorder.on_prompt(&text)
                {
                    if let Err(send_error) = sink.send(Event::Failed(error.describe())) {
                        tracing::debug!(%send_error, "session event receiver closed");
                    }
                    break;
                }
                session.push_user(text);
                let runner = Skott::with_retry(max_turns, retry);
                // Borrow the recorder only for this run; the borrow ends before the
                // next iteration, and each run starts and finishes its own record.
                let result = runner.with_limits(limits).and_then(|runner| {
                    runner.run_controlled(
                        &mut session,
                        &transport,
                        &executor,
                        &sink,
                        &cancellation,
                        &mut context,
                        borrow_recorder(&mut recorder),
                        &worker_controls,
                    )
                });
                if worker_shutdown.load(Ordering::SeqCst) {
                    break;
                }
                cancellation.reset();
                if let Err(error) = result {
                    if !worker_shutdown.load(Ordering::SeqCst)
                        && let Err(send_error) = sink.send(Event::Failed(error.describe()))
                    {
                        tracing::debug!(%send_error, "session event receiver closed");
                    }
                    // A failed recorder or broken channel must not drive later effects.
                    break;
                }
            }
            worker_controls.shutdown();
            if let Ok(mut server) = worker_server.lock() {
                server.take();
            }
        })?;
    Ok((
        SessionHandle {
            cancellation: handle_cancellation,
            thread: Some(thread),
            shutdown,
            controls,
            recording,
            control_server,
        },
        rx,
        prompt_tx,
    ))
}

/// Blocks until the next prompt arrives or every prompt sender is dropped.
/// Returns `None` when the channel closes, which ends the worker.
fn wait_for_prompt(
    controls: &ControlMailbox,
    lifetime: &std::sync::Weak<()>,
    shutdown: &AtomicBool,
) -> Option<String> {
    while !shutdown.load(Ordering::SeqCst) {
        match controls.next_task() {
            Ok(Some(prompt)) => return Some(prompt),
            Err(error) => {
                tracing::error!(%error, "submission queue unavailable");
                break;
            }
            Ok(None) => {}
        }
        if lifetime.upgrade().is_none() || controls.is_shutdown() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    None
}

/// Reborrow the recorder as `Option<&mut dyn Recorder>` for a single call.
///
/// The split borrow reborrows through the `Box`, so the mutable borrow ends
/// with the call instead of being extended across the recorder's lifetime (as a
/// direct `recorder.as_deref_mut()` borrow would be, because the value owns its
/// drop). The recorder is reused across turns, so each call needs a fresh one.
fn borrow_recorder(recorder: &mut Option<Box<dyn Recorder>>) -> Option<&mut dyn Recorder> {
    match recorder {
        Some(boxed) => Some(&mut **boxed),
        None => None,
    }
}

/// The capability must not reside in an agent-writable namespace, even when
/// a host's temporary-root environment points there.
pub(crate) fn validate_control_path(
    path: &std::path::Path,
    workdir: &std::path::Path,
) -> Result<()> {
    if path
        .to_str()
        .is_none_or(|text| text.chars().any(char::is_control))
    {
        return Err(Error::Control {
            reason: "private control socket requires a printable UTF-8 operator capability path"
                .into(),
        });
    }
    if path.canonicalize()?.starts_with(workdir) {
        return Err(Error::Control {
            reason: "private control socket must be outside the sandbox writable workspace".into(),
        });
    }
    Ok(())
}

/// Builds the default system prompt for the agent, describing its sandbox scope.
pub fn system_prompt(write_root: &str) -> String {
    format!(
        r#"
You are an autonomous coding agent operating inside a restricted execution sandbox.

## Sandbox Environment & Boundaries
- **Filesystem Access**: You may read files under `{write_root}` and allowed read-only system paths. You may ONLY create or modify files inside `{write_root}`.
- **Network Isolation**: The sandbox has NO network access. External downloads, remote fetches, and network-dependent commands will fail.
- **Toolchains**: Only the toolchains listed in the shell tool description are advertised as available. Inspect PATH and installed versions; do not assume another compiler or package manager is installed.
- **Authority**: This workspace shell is an unprivileged worker. It is NOT Kvist's protected task broker: you cannot approve tasks, mint canonical evidence, accept high-level intent, or promote workspace outputs.

## Dependency Management
- Prepared Rust dependencies are mounted **read-only** at `/rust/vendor`, outside the writable workspace. Other offline resources depend on the advertised toolchain.
- If a build or test fails due to a missing dependency, **report the missing dependency clearly** in your output. The sandbox cannot download packages; dependency provisioning happens outside the sandbox by the user running `kvist vendor`.
- **DO NOT** manually modify dependency manifests (e.g., `Cargo.toml`, `package.json`, lockfiles) or touch vendor directories to manage packages. Manual edits will cause desynchronization and break the workspace.

## Tool Protocol & File Operations
- **Exact File Edits**: Always provide the fresh SHA-256 hash returned by the most recent `read_file` when calling `edit_file`.
- **Incremental Mutations**: Prefer `edit_file` for one exact replacement, `apply_patch` for multiple exact unified hunks in one file, and `append_file` for verbatim appends. Patch/append require the current full-file SHA-256. Patch input has no file headers; all hunks validate before an atomic write.
- **Byte Offsets**: `read_file` offsets are exact UTF-8 byte positions, NOT line numbers.
  - Omitting `offset` continues that path's last successful page within this prompt, initially byte 0. You may instead pass the returned `next_offset` explicitly.
  - To re-read from the start, pass `offset: 0`. Continuation rejects changed files; restart explicitly.
  - Stop at `next_offset: null`. Further implicit reads return an empty EOF page and repeated terminal calls trigger loop detection.
  - Never describe an offset in conversational text expecting the tool to use it—pass it directly in the tool arguments.
- **Search Scope**: Restrict searches to bounded source directories or files. Do not search binary objects or executables.
- **Execution & Status**: Always check exit codes and error status returned by tools. Never assume an operation succeeded without inspecting the output.

## Workflow & Long-Session Discipline
Break complex tasks down into small, verifiable subtasks:
1. **Locate & Read**: Inspect relevant files using bounded reads.
2. **Execute**: Make focused, atomic edits using tools.
3. **Verify**: Run build/test tools after modifications to validate changes immediately.
4. **State Delta**: In one or two concise sentences, record what was accomplished before moving to the next subtask.
5. **Iterate**: Repeat until all subtasks and acceptance criteria are fully met.

- Follow established project conventions and coding standards when making changes.

{MARKDOWN_OUTPUT_GUIDANCE}"#,
        write_root = write_root,
        MARKDOWN_OUTPUT_GUIDANCE = MARKDOWN_OUTPUT_GUIDANCE
    )
}

/// Describes the explicit interactive host opt-out without claiming confinement.
pub fn host_system_prompt(write_root: &str, workdir: &std::path::Path) -> String {
    format!(
        r#"
You are a coding agent in explicit HOST UNCONFINED execution mode.
Shell commands run with real host privileges.

## Environment & Privileges
- **Working Directory**: `{workdir}`
- **File Tool Root**: `{write_root}`
- **Filesystem**: Full read/write access to the host filesystem
- **Network**: Full network access (unlike sandbox mode)
- **Toolchains**: All host-installed tools and compilers are available
- **Authority**: This workspace shell is NOT Kvist's protected task broker and cannot accept intent, approve tasks, mint canonical evidence, or promote output.

## Responsibility
Use your host privileges responsibly:
- Prefer bounded reads and exact preimage-bound edits
- Check process status and exit codes
- Be explicit about changes and their scope
- State what you did and what you changed

{MARKDOWN_OUTPUT_GUIDANCE}"#,
        write_root = write_root,
        workdir = workdir.display(),
        MARKDOWN_OUTPUT_GUIDANCE = MARKDOWN_OUTPUT_GUIDANCE
    )
}

const MARKDOWN_OUTPUT_GUIDANCE: &str = r#"
You may use any Markdown in your results, including tables, lists, links, references, and language-tagged code fences. Prefer CommonMark and GitHub-flavored Markdown for portable text. Keep tool arguments in their required structured format; Markdown is presentation, not tool execution or trusted session metadata."#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_capability_presentation_rejects_unprintable_paths_before_filesystem_access() {
        use std::os::unix::ffi::OsStrExt;
        let workdir = std::path::Path::new(".");
        for path in [
            std::path::Path::new("\u{1b}bad/control.sock"),
            std::path::Path::new(std::ffi::OsStr::from_bytes(b"/bad-\xff/control.sock")),
        ] {
            assert!(matches!(validate_control_path(path,workdir),
                Err(Error::Control{reason}) if reason.contains("printable")));
        }
    }
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    use crate::{
        ContextManager, DEFAULT_CONTEXT_TOKENS, Model, ModelProvider, RetryPolicy, RunSummary,
        ToolExecutor, ToolOutcome, ToolPolicy, ToolRegistry,
    };
    use sav::{
        CancellationToken, Error as AgentError, ModelRequest, ModelStreamEvent, ModelTransport,
        ModelTurn, ModelUsage, ReasoningEffort, ToolIntent,
    };

    /// A transport the worker never calls in these tests (no prompt is sent),
    /// so every method just reports a cancelled turn.
    struct NoopTransport;

    impl ModelTransport for NoopTransport {
        fn complete(
            &self,
            _request: &ModelRequest,
            _cancellation: &CancellationToken,
        ) -> sav::Result<ModelTurn> {
            Err(AgentError::ModelTransportCancelled)
        }

        fn stream(
            &self,
            _request: &ModelRequest,
            _cancellation: &CancellationToken,
            _on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            Err(AgentError::ModelTransportCancelled)
        }

        fn deadline(&self) -> Duration {
            Duration::from_secs(30)
        }
    }

    struct NoopExecutor;

    impl ToolExecutor for NoopExecutor {
        fn execute(
            &self,
            _intent: &ToolIntent,
            _cancellation: &CancellationToken,
        ) -> Result<ToolOutcome> {
            Ok(ToolOutcome {
                exited: true,
                status: Some(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
                timed_out: false,
                output_limit_exceeded: false,
                cancelled: false,
            })
        }
    }

    fn test_session() -> AgentSession {
        let model = Model {
            id: "test".to_owned(),
            provider: ModelProvider::LlamaServer,
            base_url: "http://127.0.0.1:1".to_owned(),
            model: "test-model".to_owned(),
            is_default: false,
            context_limit: None,
            response_reserve: None,
            deadline_secs: 30,
            max_attempts: 1,
            retry_base_delay_secs: 1,
            retry_max_delay_secs: 1,
            cadence_timeout_secs: 0,
        };
        let tool_defs = ToolRegistry::new(ToolPolicy::minimum()).tool_definitions();
        AgentSession::new(model, ReasoningEffort::None, tool_defs, "system".to_owned())
    }

    #[test]
    fn worker_exits_when_the_prompt_sender_is_dropped() {
        // The quit teardown drops the prompt sender before the session handle so
        // the worker's recv() unblocks and the thread finishes before join(). If
        // the channel could never close, join() would hang forever and force
        // Ctrl-C. Dropping the sender here and then joining proves the channel
        // closing ends the worker, which is the invariant the fix relies on.
        let context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
        let (handle, _rx, prompt_tx) = start(
            test_session(),
            NoopTransport,
            NoopExecutor,
            context,
            None,
            RetryPolicy::new(1, Duration::from_millis(1), Duration::from_millis(1)),
            1,
            RunLimits::default(),
        )
        .expect("start worker");
        drop(prompt_tx);

        // join() (inside SessionHandle::drop) must return promptly. Run it on a
        // helper thread and wait for that thread with a timeout: if the worker
        // ever hung, the helper would finish last and this assertion fails.
        let handle = handle;
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            drop(handle);
            let _ = done_tx.send(());
        });
        match done_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
            Err(RecvTimeoutError::Timeout) => {
                panic!("worker did not exit after the prompt channel closed: join hung")
            }
        }
    }

    struct PromptFailingRecorder;

    impl Recorder for PromptFailingRecorder {
        fn on_prompt(&mut self, _prompt: &str) -> Result<()> {
            Err(Error::Recording {
                reason: "injected prompt recording failure".into(),
            })
        }
        fn session_start(&mut self) -> Result<()> {
            Ok(())
        }
        fn request(&mut self, _request: &ModelRequest, _attempt: u32) -> Result<()> {
            Ok(())
        }
        fn turn_start(&mut self, _turn: &ModelTurn) -> Result<usize> {
            Ok(1)
        }
        fn turn_finish(
            &mut self,
            _turn: usize,
            _usage: &Option<ModelUsage>,
            _finish_reason: &str,
        ) -> Result<()> {
            Ok(())
        }
        fn tool_dispatch(&mut self, _turn: usize, _intent: &ToolIntent) -> Result<()> {
            Ok(())
        }
        fn tool_result(
            &mut self,
            _turn: usize,
            _call_id: &str,
            _tool: &str,
            _args: &serde_json::Value,
            _outcome: &ToolOutcome,
        ) -> Result<()> {
            Ok(())
        }
        fn session_finish(&mut self, _summary: &RunSummary) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn worker_prompt_recording_failure_fails_before_any_turn() {
        let context = ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6);
        let (handle, rx, prompt_tx) = start(
            test_session(),
            NoopTransport,
            NoopExecutor,
            context,
            Some(Box::new(PromptFailingRecorder)),
            RetryPolicy::new(1, Duration::from_millis(1), Duration::from_millis(1)),
            1,
            RunLimits::default(),
        )
        .expect("start worker");
        prompt_tx.send("fix the bug".into()).unwrap();
        let mut failed = None;
        while let Ok(event) = rx.recv() {
            if let Event::Failed(message) = &event {
                failed = Some(message.clone());
            }
        }
        assert!(
            failed
                .as_deref()
                .is_some_and(|message| message.contains("injected prompt recording failure")),
            "the recording failure surfaces as Failed: {failed:?}"
        );
        // The worker exited after the failure; the join must be prompt.
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            drop(handle);
            let _ = done_tx.send(());
        });
        assert!(
            done_rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "join hung after the prompt recording failure"
        );
    }

    #[test]
    fn worker_drop_joins_with_a_retained_prompt_sender() {
        let (handle, rx, prompt_tx) = start(
            test_session(),
            NoopTransport,
            NoopExecutor,
            ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
            None,
            RetryPolicy::new(1, Duration::from_millis(1), Duration::from_millis(1)),
            1,
            RunLimits::default(),
        )
        .unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            drop(handle);
            done_tx.send(()).unwrap();
        });
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("join must not depend on closing the prompt sender");
        assert!(prompt_tx.send("after shutdown".into()).is_err());
        drop(rx);
    }

    struct FloodTransport(Arc<std::sync::atomic::AtomicUsize>);

    struct WaitingTransport {
        started: mpsc::Sender<()>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl ModelTransport for WaitingTransport {
        fn complete(&self, _: &ModelRequest, token: &CancellationToken) -> sav::Result<ModelTurn> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.send(()).unwrap();
            while !token.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Err(AgentError::ModelTransportCancelled)
        }

        fn stream(
            &self,
            request: &ModelRequest,
            token: &CancellationToken,
            _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            self.complete(request, token)
        }

        fn deadline(&self) -> Duration {
            Duration::from_secs(60)
        }
    }

    #[test]
    fn shutdown_cancels_active_provider_without_starting_queued_prompts() {
        let (started, ready) = mpsc::channel();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (handle, rx, prompt_tx) = start(
            test_session(),
            WaitingTransport {
                started,
                calls: calls.clone(),
            },
            NoopExecutor,
            ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
            None,
            RetryPolicy::new(1, Duration::from_millis(1), Duration::from_millis(1)),
            1,
            RunLimits::default(),
        )
        .unwrap();
        prompt_tx.send("active".into()).unwrap();
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        prompt_tx.send("must not start".into()).unwrap();
        let started = std::time::Instant::now();
        handle.shutdown();
        drop(handle);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(prompt_tx.send("after shutdown".into()).is_err());
        drop(rx);
    }

    impl ModelTransport for FloodTransport {
        fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
            Err(AgentError::ModelTransportCancelled)
        }
        fn stream(
            &self,
            _: &ModelRequest,
            _: &CancellationToken,
            emit: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            for index in 1..=1000 {
                self.0.store(index, Ordering::SeqCst);
                emit(ModelStreamEvent::TextDelta("provisional".into()))?;
            }
            Err(AgentError::ModelTransportCancelled)
        }
        fn deadline(&self) -> Duration {
            Duration::from_secs(30)
        }
    }

    #[test]
    fn worker_drop_joins_when_the_event_queue_is_full() {
        let attempted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (handle, rx, prompt_tx) = start(
            test_session(),
            FloodTransport(Arc::clone(&attempted)),
            NoopExecutor,
            ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
            None,
            RetryPolicy::new(1, Duration::from_millis(1), Duration::from_millis(1)),
            1,
            RunLimits::default(),
        )
        .unwrap();
        prompt_tx.send("flood".into()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while attempted.load(Ordering::SeqCst) < 127 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(
            attempted.load(Ordering::SeqCst) >= 127,
            "fill the 128-slot queue before teardown"
        );
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            drop(handle);
            done_tx.send(()).unwrap();
        });
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("backpressure must not prevent join");
        drop((rx, prompt_tx));
    }

    struct GuidedTransport {
        started: mpsc::Sender<()>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl ModelTransport for GuidedTransport {
        fn complete(&self, _: &ModelRequest, _: &CancellationToken) -> sav::Result<ModelTurn> {
            unreachable!()
        }

        fn stream(
            &self,
            request: &ModelRequest,
            token: &CancellationToken,
            _: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                self.started.send(()).unwrap();
                while !token.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
                return Err(AgentError::ModelTransportCancelled);
            }
            assert!(
                request
                    .messages
                    .contains(&sav::ModelMessage::User("original objective".into()))
            );
            assert!(
                request
                    .messages
                    .contains(&sav::ModelMessage::User("additional guidance".into()))
            );
            Ok(ModelTurn {
                text: "guided completion".into(),
                reasoning: None,
                tool_intents: vec![],
                finish_reason: sav::FinishReason::Stop,
                provider: sav::LocalModelProvider::LlamaServer,
                model: "test".into(),
                response_id: None,
                provider_request_id: None,
                usage: None,
            })
        }

        fn deadline(&self) -> Duration {
            Duration::from_secs(5)
        }
    }

    #[test]
    fn real_submission_path_interrupts_active_generation_instead_of_queuing_a_task() {
        let (started, ready) = mpsc::channel();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (handle, receiver, prompts) = start(
            test_session(),
            GuidedTransport {
                started,
                calls: calls.clone(),
            },
            NoopExecutor,
            ContextManager::new(DEFAULT_CONTEXT_TOKENS, 6),
            None,
            RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(1)),
            5,
            RunLimits::default(),
        )
        .unwrap();
        assert_eq!(
            prompts.send("original objective".into()).unwrap().kind,
            crate::SubmissionKind::Task
        );
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        let guidance = prompts.send("additional guidance".into()).unwrap();
        assert_eq!(guidance.kind, crate::SubmissionKind::Guidance);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut states = Vec::new();
        loop {
            let event = receiver
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .unwrap();
            match event {
                Event::Control(notice) => {
                    assert_eq!(notice.id, guidance.id);
                    states.push(notice.state);
                }
                Event::Finished { message } => {
                    assert_eq!(message, "guided completion");
                    break;
                }
                Event::Failed(message) => panic!("{message}"),
                _ => {}
            }
        }
        assert_eq!(
            states,
            vec![
                crate::GuidanceState::Accepted,
                crate::GuidanceState::Claimed,
                crate::GuidanceState::Delivered
            ]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(!handle.controls().is_active());
        assert!(prompts.steer("not a replacement task".into()).is_err());
        assert_eq!(handle.controls().next_task().unwrap(), None);
        assert!(
            receiver.recv_timeout(Duration::from_millis(30)).is_err(),
            "guidance must never silently start a replacement task"
        );
        handle.shutdown();
    }

    #[test]
    fn host_prompt_does_not_claim_sandbox_confinement() {
        let prompt = host_system_prompt("/workspace", std::path::Path::new("/tmp/project"));
        assert!(prompt.contains("HOST UNCONFINED") && prompt.contains("/tmp/project"));
        assert!(prompt.contains("NOT Kvist's protected task broker"));
        assert!(!prompt.contains("running inside a sandbox"));
    }

    #[test]
    fn both_execution_scopes_permit_markdown_results() {
        for prompt in [
            system_prompt("/workspace"),
            host_system_prompt("/workspace", std::path::Path::new("/tmp/project")),
        ] {
            for feature in [
                "Markdown",
                "tables",
                "lists",
                "links",
                "references",
                "code fences",
            ] {
                assert!(prompt.contains(feature), "missing {feature}: {prompt}");
            }
        }
    }
}
