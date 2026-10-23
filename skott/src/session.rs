//! Skott adapters for Sav's host-neutral native runtime.
use crate::{
    config::Model,
    context::ContextManager,
    error::{Error, Result},
    retry::RetryPolicy,
    sandbox::ToolOutcome,
};
pub use sav::runtime::{
    ControlMailbox, Event, GuidanceId, GuidanceNotice, GuidanceState, MAX_TOOL_RESULT_BYTES,
    MAX_TURNS, RunLimits, RunSummary, Submission, SubmissionKind, TokenAccounting,
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
    pub max_turns: u32,
    pub retry: RetryPolicy,
    pub limits: RunLimits,
}
impl Default for Skott {
    fn default() -> Self {
        Self::with_retry(MAX_TURNS, RetryPolicy::default())
    }
}
impl Skott {
    pub fn with_retry(max_turns: u32, retry: RetryPolicy) -> Self {
        Self {
            max_turns,
            retry,
            limits: RunLimits::default(),
        }
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
            .with_limits(self.limits)?;
        let mut recorder = recorder.map(RecordBridge);
        let shared_recorder = recorder
            .as_mut()
            .map(|r| r as &mut dyn sav::runtime::Recorder);
        runner
            .run_controlled(
                &mut session.0,
                transport,
                &ExecutorBridge(executor),
                &SinkBridge(sink),
                cancellation,
                &mut context.0,
                shared_recorder,
                controls,
            )
            .map_err(Error::from)
    }
}
struct ExecutorBridge<'a, E>(&'a E);
impl<E: ToolExecutor> sav::runtime::ToolExecutor for ExecutorBridge<'_, E> {
    fn execute(
        &self,
        intent: &ToolIntent,
        token: &CancellationToken,
    ) -> sav::runtime::Result<ToolOutcome> {
        self.0.execute(intent, token).map_err(Into::into)
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
    fn repeated_observation_notice(&self, intent: &ToolIntent) -> Option<String> {
        (intent.name == "read_file" && intent.arguments.get("offset").is_none()).then(||
            "Repeated read_file omitted offset, so it requests byte page 0 again. To continue, supply the previous result's actual next_offset; offsets are bytes, not lines.".into())
    }
}
struct SinkBridge<'a, S>(&'a S);
impl<S: EventSink> sav::runtime::EventSink for SinkBridge<'_, S> {
    fn send(&self, event: Event) -> sav::runtime::Result<()> {
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
