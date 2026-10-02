//! The worker thread that runs a session loop off the UI thread.
//!
//! The model transport and sandbox executor are blocking, so the loop runs on a
//! owned thread and pushes [`Event`]s over a bounded channel. The UI owns the
//! channel receiver and renders each event. Cancellation flows from a shared
//! [`CancellationToken`], and new prompts flow from the UI over a prompt channel.

use std::sync::mpsc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use agent_runtime::{CancellationToken, ModelTransport};

use crate::context::ContextManager;
use crate::error::{Error, Result};
use crate::retry::RetryPolicy;
use crate::session::{
    AgentRunner, AgentSession, Event, EventSink, Recorder, RunLimits, ToolExecutor,
};

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
}

impl SessionHandle {
    /// Requests cancellation of the running turn.
    pub fn cancel(&self) {
        self.cancellation.cancel();
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
        self.cancel();
        self.shutdown.store(true, Ordering::SeqCst);
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
    mut session: AgentSession,
    transport: M,
    executor: E,
    mut context: ContextManager,
    mut recorder: Option<Box<dyn Recorder>>,
    retry: RetryPolicy,
    max_turns: u32,
    limits: RunLimits,
) -> Result<(SessionHandle, mpsc::Receiver<Event>, mpsc::Sender<String>)>
where
    M: ModelTransport + Send + 'static,
    E: ToolExecutor + Send + 'static,
{
    let (prompt_tx, prompt_rx) = mpsc::channel::<String>();
    let (tx, rx) = mpsc::sync_channel(128);
    let cancellation = CancellationToken::new();
    let handle_cancellation = cancellation.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker_shutdown = Arc::clone(&shutdown);
    let thread = thread::Builder::new()
        .name("agent-session".into())
        .spawn(move || {
            let sink = ChannelSink {
                sender: tx,
                shutdown: Arc::clone(&worker_shutdown),
            };
            // The worker stays alive until shutdown or prompt disconnection.
            // `AgentRunner::run` records each prompt's session
            // boundary in the durable recorder, so the worker does not manage the
            // record lifecycle itself. Cancellation re-arms the token after each
            // turn so a cancelled turn does not end the session.
            while let Some(text) = wait_for_prompt(&prompt_rx, &worker_shutdown) {
                session.push_user(text);
                let runner = AgentRunner::with_retry(max_turns, retry);
                // Borrow the recorder only for this run; the borrow ends before the
                // next iteration, and each run starts and finishes its own record.
                let result = runner.with_limits(limits).and_then(|runner| {
                    runner.run(
                        &mut session,
                        &transport,
                        &executor,
                        &sink,
                        &cancellation,
                        &mut context,
                        borrow_recorder(&mut recorder),
                    )
                });
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
        })?;
    Ok((
        SessionHandle {
            cancellation: handle_cancellation,
            thread: Some(thread),
            shutdown,
        },
        rx,
        prompt_tx,
    ))
}

/// Blocks until the next prompt arrives or every prompt sender is dropped.
/// Returns `None` when the channel closes, which ends the worker.
fn wait_for_prompt(rx: &mpsc::Receiver<String>, shutdown: &AtomicBool) -> Option<String> {
    while !shutdown.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(prompt) => return Some(prompt),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
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

/// Builds the default system prompt for the agent, describing its sandbox scope.
pub fn system_prompt(write_root: &str) -> String {
    format!(
        "You are a coding agent running inside a sandbox. The working directory is mounted at {write_root} \
         and is the only place you may write. You can read files under {write_root} and the read-only system \
         layout. This workspace shell is NOT Kvist's protected task broker and cannot accept intent, \
         approve tasks, mint canonical evidence or promote output. Network is denied for tools. \
         Prefer bounded reads/searches and exact edit_file with the SHA-256 returned by read_file. \
         read_file offsets are UTF-8 byte positions, not line numbers: pass actual next_offset to \
         continue a page. Omitting offset intentionally reads page zero; describing an offset in prose \
         does not supply it as a tool argument. Use directory or regular-file search scopes and \
         bounded source filters; binary executables are not source text. \
         Tools return process status; check failures. Prefer small, reversible steps. State what you did."
    )
}

/// Describes the explicit interactive host opt-out without claiming confinement.
pub fn host_system_prompt(write_root: &str, workdir: &std::path::Path) -> String {
    format!(
        "You are a coding agent in explicit HOST UNCONFINED execution mode. Shell commands run with \
         real host privileges from {}. Host writes and network are NOT sandbox constrained. \
         File-tool paths use {write_root}, mapped to that working directory. \
         This workspace shell is NOT Kvist's protected task broker and cannot accept intent, \
         approve tasks, mint canonical evidence or promote output. Prefer bounded reads and \
         exact preimage-bound edits. Check process status and state what you did.",
        workdir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    use crate::{
        ContextManager, DEFAULT_CONTEXT_TOKENS, Model, ModelProvider, RetryPolicy, ToolExecutor,
        ToolOutcome, ToolPolicy, ToolRegistry,
    };
    use agent_runtime::{
        CancellationToken, Error as AgentError, ModelRequest, ModelStreamEvent, ModelTransport,
        ModelTurn, ReasoningEffort, ToolIntent,
    };

    /// A transport the worker never calls in these tests (no prompt is sent),
    /// so every method just reports a cancelled turn.
    struct NoopTransport;

    impl ModelTransport for NoopTransport {
        fn complete(
            &self,
            _request: &ModelRequest,
            _cancellation: &CancellationToken,
        ) -> agent_runtime::Result<ModelTurn> {
            Err(AgentError::ModelTransportCancelled)
        }

        fn stream(
            &self,
            _request: &ModelRequest,
            _cancellation: &CancellationToken,
            _on_event: &mut dyn FnMut(ModelStreamEvent) -> agent_runtime::Result<()>,
        ) -> agent_runtime::Result<ModelTurn> {
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

    impl ModelTransport for FloodTransport {
        fn complete(
            &self,
            _: &ModelRequest,
            _: &CancellationToken,
        ) -> agent_runtime::Result<ModelTurn> {
            Err(AgentError::ModelTransportCancelled)
        }
        fn stream(
            &self,
            _: &ModelRequest,
            _: &CancellationToken,
            emit: &mut dyn FnMut(ModelStreamEvent) -> agent_runtime::Result<()>,
        ) -> agent_runtime::Result<ModelTurn> {
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

    #[test]
    fn host_prompt_does_not_claim_sandbox_confinement() {
        let prompt = host_system_prompt("/workspace", std::path::Path::new("/tmp/project"));
        assert!(prompt.contains("HOST UNCONFINED") && prompt.contains("/tmp/project"));
        assert!(prompt.contains("NOT sandbox constrained"));
        assert!(!prompt.contains("running inside a sandbox"));
    }
}
