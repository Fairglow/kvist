//! The worker thread that runs a session loop off the UI thread.
//!
//! The model transport and sandbox executor are blocking, so the loop runs on a
//! detached thread and pushes [`Event`]s over a bounded channel. The UI owns the
//! channel receiver and renders each event. Cancellation flows from a shared
//! [`CancellationToken`], and new prompts flow from the UI over a prompt channel.

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use agent_runtime::{CancellationToken, ModelTransport};

use crate::context::ContextManager;
use crate::error::{Error, Result};
use crate::retry::RetryPolicy;
use crate::session::{AgentRunner, AgentSession, Event, EventSink, Recorder, ToolExecutor};

/// Receives [`Event`]s from the worker; implements [`EventSink`].
struct ChannelSink(mpsc::SyncSender<Event>);

impl EventSink for ChannelSink {
    fn send(&self, event: Event) -> Result<()> {
        self.0.send(event).map_err(|_| Error::ChannelClosed)
    }
}

/// A handle to a running session, holding the cancellation token and thread.
pub struct SessionHandle {
    cancellation: CancellationToken,
    thread: Option<JoinHandle<()>>,
}

impl SessionHandle {
    /// Requests cancellation of the running turn.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Reports whether a turn is still running.
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.thread.is_some()
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
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
pub fn start<M, E>(
    mut session: AgentSession,
    transport: M,
    executor: E,
    mut context: ContextManager,
    mut recorder: Option<Box<dyn Recorder>>,
    retry: RetryPolicy,
    max_turns: u32,
) -> (SessionHandle, mpsc::Receiver<Event>, mpsc::Sender<String>)
where
    M: ModelTransport + Send + 'static,
    E: ToolExecutor + Send + 'static,
{
    let (prompt_tx, prompt_rx) = mpsc::channel::<String>();
    let (tx, rx) = mpsc::sync_channel(128);
    let cancellation = CancellationToken::new();
    let handle_cancellation = cancellation.clone();
    let thread = thread::spawn(move || {
        let sink = ChannelSink(tx);
        // The worker stays alive across turns; it exits only when every prompt
        // sender is dropped. `AgentRunner::run` records each turn's session
        // boundary in the durable recorder, so the worker does not manage the
        // record lifecycle itself. Cancellation re-arms the token after each
        // turn so a cancelled turn does not end the session.
        while let Some(text) = wait_for_prompt(&prompt_rx) {
            session.push_user(text);
            let runner = AgentRunner::with_retry(max_turns, retry);
            // Borrow the recorder only for this run; the borrow ends before the
            // next iteration, and each run starts and finishes its own record.
            let _ = runner
                .run(
                    &mut session,
                    &transport,
                    &executor,
                    &sink,
                    &cancellation,
                    &mut context,
                    borrow_recorder(&mut recorder),
                )
                .unwrap_or_else(|_| crate::session::RunSummary::default());
            cancellation.reset();
        }
    });
    (
        SessionHandle {
            cancellation: handle_cancellation,
            thread: Some(thread),
        },
        rx,
        prompt_tx,
    )
}

/// Blocks until the next prompt arrives or every prompt sender is dropped.
/// Returns `None` when the channel closes, which ends the worker.
fn wait_for_prompt(rx: &mpsc::Receiver<String>) -> Option<String> {
    rx.recv().ok()
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
         layout. Prefer small, reversible steps. When editing files, write complete files. State what you did."
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
        );
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
}
