//! Skott adapters for Sav's host-neutral native runtime.
mod read_cursor;

use std::sync::Mutex;

use crate::{
    config::Model,
    context::ContextManager,
    error::{Error, Result},
    retry::RetryPolicy,
    sandbox::ToolOutcome,
};
pub use sav::loop_detection::ProgressEvidence;
pub use sav::runtime::{
    BudgetResource, ControlMailbox, EffectCertainty, Event, GuidanceId, GuidanceNotice,
    GuidanceState, MAX_TOOL_RESULT_BYTES, MAX_TURNS, RecoveryAction, RunLimits, RunSummary,
    StopReason, Submission, SubmissionKind, TokenAccounting, ToolFailure, ToolFailureCategory,
};
use sav::{
    CancellationToken, ModelRequest, ModelTransport, ModelTurn, ModelUsage, ReasoningEffort,
    ToolDefinition, ToolIntent,
};

/// Application event sink, preserving Skott's typed error boundary.
pub trait EventSink: Send {
    fn send(&self, event: Event) -> Result<()>;
}

/// Selected-model conversation backed entirely by Sav.
pub struct AgentSession(pub(crate) sav::runtime::AgentSession);
impl AgentSession {
    pub fn new(
        model: Model,
        effort: ReasoningEffort,
        tools: Vec<ToolDefinition>,
        system: String,
    ) -> Self {
        Self(sav::runtime::AgentSession::new(
            sav::runtime::ModelIdentity {
                id: model.id,
                model: model.model,
            },
            effort,
            tools,
            system,
        ))
    }
}
impl std::ops::Deref for AgentSession {
    type Target = sav::runtime::AgentSession;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for AgentSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
/// Executes a tool intent and returns its sandbox outcome. Injected into the
/// loop so the loop itself needs no process or filesystem access. The executor
/// owns the working directory and the sandbox configuration.
pub trait ToolExecutor: Send + Sync {
    /// Renders and executes one tool intent inside the sandbox.
    fn execute(&self, intent: &ToolIntent, cancellation: &CancellationToken)
    -> Result<ToolOutcome>;

    /// Returns host-observed state change for this exact execution. Success or
    /// command output alone is not evidence; generic executors make no claim.
    fn progress(&self, _intent: &ToolIntent, _outcome: &ToolOutcome) -> Result<ProgressEvidence> {
        Ok(ProgressEvidence::None)
    }
}

/// A durable, pluggable sink for the session record. The production
/// implementation ([`crate::session_log::SessionLog`]) writes a structured
/// journal and a readable transcript; tests can record in memory. Recording the
/// full reasoning trace here is what keeps thinking inspectable even after
/// compaction removes it from the model context.
pub trait Recorder: Send {
    /// Records a provisional live provider event before UI delivery.
    fn model_event(&mut self, _event: &sav::ModelStreamEvent) -> Result<()> {
        Ok(())
    }
    /// Persists correlated control lifecycle observations.
    fn control(&mut self, notice: &GuidanceNotice) -> Result<()> {
        self.notice(&format!("guidance #{}: {:?}", notice.id.0, notice.state))
    }
    /// Records a bounded operational notice; never authorizes an effect.
    fn notice(&mut self, _message: &str) -> Result<()> {
        Ok(())
    }
    /// Records a bounded typed failure before any later effects.
    fn tool_failure(&mut self, failure: &ToolFailure) -> Result<()> {
        self.notice(&crate::operational::failure_text("call", failure))
    }
    /// Called once per prompt as it is received by the worker, before any turn.
    /// The durable session log uses it to label the transcript after the first
    /// prompt; other recorders ignore it.
    fn on_prompt(&mut self, _prompt: &str) -> Result<()> {
        Ok(())
    }
    /// Writes the session-start event. Called once, before the first turn.
    fn session_start(&mut self) -> Result<()>;
    /// Records a model attempt, without granting execution authority.
    fn request(&mut self, request: &ModelRequest, attempt: u32) -> Result<()>;
    /// Begins one turn, capturing its reasoning trace. Returns the turn index
    /// used by later tool records.
    fn turn_start(&mut self, turn: &ModelTurn) -> Result<usize>;
    /// Completes one turn, folding in provider token usage.
    fn turn_finish(
        &mut self,
        turn: usize,
        usage: &Option<ModelUsage>,
        finish_reason: &str,
    ) -> Result<()>;
    /// Synchronizes a dispatch intent before the executor may cause effects.
    fn tool_dispatch(&mut self, turn: usize, intent: &ToolIntent) -> Result<()>;
    /// Records an approved tool call and its sandbox outcome.
    fn tool_result(
        &mut self,
        turn: usize,
        call_id: &str,
        tool: &str,
        args: &serde_json::Value,
        outcome: &ToolOutcome,
    ) -> Result<()>;
    /// Writes the terminal session-finish event with session totals.
    fn session_finish(&mut self, summary: &RunSummary) -> Result<()>;
}

/// Thin application adapter for the shared bounded native loop.
pub struct Skott {
    pub max_turns: Option<u32>,
    pub retry: RetryPolicy,
    pub limits: RunLimits,
    generation_loop_detection: bool,
}
impl Default for Skott {
    fn default() -> Self {
        Self::with_retry(None, RetryPolicy::default())
    }
}
impl Skott {
    pub fn with_retry(max_turns: impl Into<Option<u32>>, retry: RetryPolicy) -> Self {
        Self {
            max_turns: max_turns.into(),
            retry,
            limits: RunLimits::default(),
            generation_loop_detection: true,
        }
    }
    /// Host-owned exception for intentional repetition; effects remain protected.
    pub fn with_generation_loop_detection(mut self, enabled: bool) -> Self {
        self.generation_loop_detection = enabled;
        self
    }
    pub fn with_limits(mut self, limits: RunLimits) -> Result<Self> {
        self.limits = limits.validate()?;
        Ok(self)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn run<M: ModelTransport, E: ToolExecutor, S: EventSink>(
        &self,
        session: &mut AgentSession,
        transport: &M,
        executor: &E,
        sink: &S,
        cancellation: &CancellationToken,
        context: &mut ContextManager,
        recorder: Option<&mut dyn Recorder>,
    ) -> Result<RunSummary> {
        self.run_controlled(
            session,
            transport,
            executor,
            sink,
            cancellation,
            context,
            recorder,
            &ControlMailbox::default(),
        )
    }
    /// Uses the same steering capability as interactive worker submissions.
    #[allow(clippy::too_many_arguments)]
    pub fn run_controlled<M: ModelTransport, E: ToolExecutor, S: EventSink>(
        &self,
        session: &mut AgentSession,
        transport: &M,
        executor: &E,
        sink: &S,
        cancellation: &CancellationToken,
        context: &mut ContextManager,
        recorder: Option<&mut dyn Recorder>,
        controls: &ControlMailbox,
    ) -> Result<RunSummary> {
        let runner = sav::runtime::NativeRuntime::with_retry(self.max_turns, self.retry)
            .with_limits(self.limits)?
            .with_generation_loop_detection(self.generation_loop_detection);
        let mut recorder = recorder.map(RecordBridge);
        let shared_recorder = recorder
            .as_mut()
            .map(|r| r as &mut dyn sav::runtime::Recorder);
        runner
            .run_controlled(
                &mut session.0,
                transport,
                &ExecutorBridge {
                    executor,
                    reads: Mutex::new(read_cursor::ReadCursors::default()),
                    executed: Mutex::new(None),
                },
                &SinkBridge(sink),
                cancellation,
                &mut context.0,
                shared_recorder,
                controls,
            )
            .map_err(Error::from)
    }
}
struct ExecutorBridge<'a, E> {
    executor: &'a E,
    reads: Mutex<read_cursor::ReadCursors>,
    // The shared runtime executes and observes one call at a time.
    executed: Mutex<Option<ExecutedObservation>>,
}

struct ExecutedObservation {
    intent: ToolIntent,
    progress: Result<ProgressEvidence>,
}

fn observation_error(intent: &ToolIntent, error: Error) -> Error {
    Error::SandboxExec {
        tool: Some(intent.name.clone()),
        reason: format!(
            "completed executor outcome retained; observation validation failed: {}; inspect state and reconcile before further work; no automatic replay",
            error.describe()
        ),
    }
}

impl<E> ExecutorBridge<'_, E> {
    fn reads(&self) -> Result<std::sync::MutexGuard<'_, read_cursor::ReadCursors>> {
        self.reads.lock().map_err(|_| Error::ToolRender {
            tool: "read_file".into(),
            reason: "prompt read cursor state is poisoned".into(),
        })
    }

    fn executed(&self) -> Result<std::sync::MutexGuard<'_, Option<ExecutedObservation>>> {
        self.executed.lock().map_err(|_| Error::SandboxExec {
            tool: None,
            reason: "resolved tool observation state is poisoned".into(),
        })
    }
}

impl<E: ToolExecutor> sav::runtime::ToolExecutor for ExecutorBridge<'_, E> {
    fn execute(
        &self,
        intent: &ToolIntent,
        token: &CancellationToken,
    ) -> sav::runtime::Result<ToolOutcome> {
        let resolved = self.reads()?.resolve(intent)?;
        let mut executed = self.executed()?;
        let outcome = self
            .executor
            .execute(&resolved, token)
            .map_err(|error| match error {
                Error::SandboxUnavailable { .. }
                | Error::ToolchainUnavailable { .. }
                | Error::SandboxBuild { .. } => Error::ToolPrerequisite {
                    tool: resolved.name.clone(),
                    reason: error.describe(),
                },
                other => other,
            })?;
        // Validation belongs to observation, after the completed outcome is folded
        // and recorded. It cannot replace actual process bytes with a refusal.
        let progress = self
            .reads()
            .and_then(|mut reads| reads.observe(&resolved, &outcome))
            .map_err(|error| observation_error(&resolved, error));
        *executed = Some(ExecutedObservation {
            intent: resolved,
            progress,
        });
        Ok(outcome)
    }
    fn progress(
        &self,
        intent: &ToolIntent,
        outcome: &ToolOutcome,
    ) -> sav::runtime::Result<ProgressEvidence> {
        let (resolved, read_progress) = match self.executed()?.take() {
            Some(ExecutedObservation { intent, progress }) => (intent, progress?),
            None => (intent.clone(), ProgressEvidence::None),
        };
        let progress = self
            .executor
            .progress(&resolved, outcome)
            .map_err(|error| observation_error(&resolved, error))?;
        Ok(if read_progress == ProgressEvidence::StateChanged {
            read_progress
        } else {
            progress
        })
    }
    fn describe(&self, intent: &ToolIntent) -> String {
        crate::tools::describe_tool_call(intent)
    }
    fn repeatable(&self, intent: &ToolIntent) -> bool {
        matches!(
            intent.name.as_str(),
            "read_file" | "list_dir" | "find_files" | "search_files"
        )
    }
}
struct SinkBridge<'a, S>(&'a S);
impl<S: EventSink> sav::runtime::EventSink for SinkBridge<'_, S> {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
        let event = match event {
            Event::ToolFailure {
                description,
                name,
                failure,
            } => Event::ToolFailure {
                description: crate::operational::printable(&description),
                name: crate::operational::printable(&name),
                failure: crate::operational::safe_failure(&failure),
            },
            Event::Failed(message) => Event::Failed(crate::operational::printable(&message)),
            other => other,
        };
        self.0.send(event).map_err(Into::into)
    }
}
struct RecordBridge<'a>(&'a mut dyn Recorder);
impl sav::runtime::Recorder for RecordBridge<'_> {
    fn model_event(&mut self, event: &sav::ModelStreamEvent) -> sav::runtime::Result<()> {
        self.0.model_event(event).map_err(Into::into)
    }
    fn notice(&mut self, message: &str) -> sav::runtime::Result<()> {
        self.0.notice(message).map_err(Into::into)
    }
    fn tool_failure(&mut self, failure: &ToolFailure) -> sav::runtime::Result<()> {
        self.0
            .tool_failure(&crate::operational::safe_failure(failure))
            .map_err(Into::into)
    }
    fn control(&mut self, notice: &GuidanceNotice) -> sav::runtime::Result<()> {
        self.0.control(notice).map_err(Into::into)
    }
    fn on_prompt(&mut self, prompt: &str) -> sav::runtime::Result<()> {
        self.0.on_prompt(prompt).map_err(Into::into)
    }
    fn session_start(&mut self) -> sav::runtime::Result<()> {
        self.0.session_start().map_err(Into::into)
    }
    fn request(&mut self, request: &ModelRequest, attempt: u32) -> sav::runtime::Result<()> {
        self.0.request(request, attempt).map_err(Into::into)
    }
    fn turn_start(&mut self, turn: &ModelTurn) -> sav::runtime::Result<usize> {
        self.0.turn_start(turn).map_err(Into::into)
    }
    fn turn_finish(
        &mut self,
        turn: usize,
        usage: &Option<ModelUsage>,
        reason: &str,
    ) -> sav::runtime::Result<()> {
        self.0.turn_finish(turn, usage, reason).map_err(Into::into)
    }
    fn tool_dispatch(&mut self, turn: usize, intent: &ToolIntent) -> sav::runtime::Result<()> {
        self.0.tool_dispatch(turn, intent).map_err(Into::into)
    }
    fn tool_result(
        &mut self,
        turn: usize,
        id: &str,
        tool: &str,
        args: &serde_json::Value,
        outcome: &ToolOutcome,
    ) -> sav::runtime::Result<()> {
        self.0
            .tool_result(turn, id, tool, args, outcome)
            .map_err(Into::into)
    }
    fn session_finish(&mut self, summary: &RunSummary) -> sav::runtime::Result<()> {
        self.0.session_finish(summary).map_err(Into::into)
    }
}
