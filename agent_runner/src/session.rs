//! The transport-agnostic agent session and multi-turn loop.
//!
//! [`AgentSession`] owns the ordered conversation and builds turns.
//! [`AgentRunner::run`] drives the loop against any [`ModelTransport`], executes
//! the tool intents via a [`ToolExecutor`], and forwards progress through an
//! [`EventSink`]. Nothing here performs blocking subprocess I/O directly; the
//! executor is injected so the loop is unit-testable with fakes.

use std::collections::HashSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use agent_runtime::{
    CancellationToken, ModelMessage, ModelRequest, ModelStreamEvent, ModelTransport, ModelTurn,
    ModelUsage, ReasoningEffort, ToolChoice, ToolDefinition, ToolIntent,
};

use crate::config::Model;
use crate::context::ContextManager;
use crate::error::{Error, Result};
use crate::retry::RetryPolicy;
use crate::sandbox::ToolOutcome;

/// The maximum number of model turns in one session before the loop stops.
pub const MAX_TURNS: u32 = 50;
/// The maximum bytes of a tool result folded back to the model.
pub const MAX_TOOL_RESULT_BYTES: usize = 8 * 1024;

/// Minimum gap between live progress updates emitted while a turn streams, so
/// the stats bar tracks progress without flooding the sink on every token.
const LIVE_PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Read-only, turn-scoped state the streaming loop uses to emit live progress.
///
/// The provider's token usage is not known until the turn ends, so the running
/// output estimate is derived from the characters streamed so far; the context
/// figures come from the session's live context, which does not change mid-turn.
struct LiveProgress<'a> {
    session: &'a AgentSession,
    context: &'a ContextManager,
    tool_defs: usize,
    started_at: Instant,
    cumulative_total: u64,
}

/// Emits a single live progress update while a turn streams, so the stats bar
/// shows working speed, context, and elapsed time as the model generates rather
/// than only after the turn completes. Best-effort: a closed sink only means the
/// UI is gone. The running output is estimated from streamed characters because
/// the provider reports usage only when the turn ends.
fn emit_live_progress<S: EventSink>(
    sink: &S,
    progress: &LiveProgress,
    output_chars: u64,
    at: Instant,
) -> Result<()> {
    let output_tokens = output_chars.div_ceil(4);
    let elapsed = at
        .saturating_duration_since(progress.started_at)
        .as_secs_f64()
        .max(1e-9);
    let total_tokens = progress.cumulative_total.saturating_add(output_tokens);
    sink.send(Event::Progress {
        input_tokens: progress.cumulative_total,
        output_tokens,
        context_tokens: progress.session.estimate_context_tokens(progress.tool_defs),
        context_limit: progress.context.limit_tokens(),
        context_utilization: progress
            .session
            .utilization(progress.context, progress.tool_defs),
        compaction_progress: progress
            .session
            .compaction_progress(progress.context, progress.tool_defs),
        tokens_per_sec: total_tokens as f64 / elapsed,
        total_tokens,
        elapsed_secs: elapsed,
    })
}

/// A progress event emitted while a session runs.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// A model turn began.
    TurnStart { model: String },
    /// A fresh model attempt; streamed text remains provisional until completion.
    AttemptStart { attempt: u32 },
    /// A fragment of model reasoning text.
    Reasoning(String),
    /// A fragment of model answer text.
    Text(String),
    /// A tool call was proposed by the model, with a short human description of
    /// what it applies to (the file, directory, or command).
    ToolCall { description: String, name: String },
    /// A tool call finished; `failed` marks a non-zero or non-exiting result.
    ToolResult {
        description: String,
        name: String,
        failed: bool,
    },
    /// The session produced a final answer.
    Finished { message: String },
    /// The prompt loop has exited and control has returned to the caller, even
    /// though the model produced no answer. Emitted on every loop exit that
    /// `Finished` does not already cover (the single-turn tool cap, or a user
    /// cancellation before a tool ran) so the UI always learns control has
    /// returned and stops showing "working…"; without it `running` would stay
    /// `true` forever and the UI looks stuck after the single-turn cap cuts a
    /// prompt off. `exhausted` marks a cap cutoff; `cancelled` marks a cancel.
    PromptEnd { exhausted: bool, cancelled: bool },
    /// The session failed before producing an answer.
    Failed(String),
    /// A non-terminal notice (for example, a compaction that trimmed the model
    /// context). Rendered as a muted line rather than an error.
    Note(String),
    /// Periodic context/token accounting, emitted after each turn, feeding the
    /// live speed stat, the context bargraph, and the compaction progress bar.
    Progress {
        /// Cumulative input tokens so far.
        input_tokens: u64,
        /// Cumulative output tokens so far.
        output_tokens: u64,
        /// Tokens the model currently holds in context.
        context_tokens: usize,
        /// The model context limit, in tokens.
        context_limit: usize,
        /// Context utilization against the limit (`0.0..=1.0+`).
        context_utilization: f64,
        /// How close compaction is to the hard limit (`0.0..=1.0`).
        compaction_progress: f64,
        /// Session-wide tokens per second.
        tokens_per_sec: f64,
        /// Cumulative provider tokens observed across the session.
        total_tokens: u64,
        /// Wall-clock seconds since the run started.
        elapsed_secs: f64,
    },
}

/// The sink that receives [`Event`]s, typically a channel to the UI.
pub trait EventSink: Send {
    /// Forwards one event; an error (e.g. a closed channel) stops the loop.
    fn send(&self, event: Event) -> Result<()>;
}

/// The transport-agnostic conversation state.
pub struct AgentSession {
    model: Model,
    thinking_effort: ReasoningEffort,
    system_prompt: String,
    messages: Vec<ModelMessage>,
    tool_defs: Vec<ToolDefinition>,
    answer: Option<String>,
}

impl AgentSession {
    /// Creates an empty session for one model.
    pub fn new(
        model: Model,
        thinking_effort: ReasoningEffort,
        tool_defs: Vec<ToolDefinition>,
        system_prompt: String,
    ) -> Self {
        AgentSession {
            model,
            thinking_effort,
            system_prompt,
            messages: Vec::new(),
            tool_defs,
            answer: None,
        }
    }

    /// Appends a user message. The session then has pending work.
    pub fn push_user(&mut self, text: impl Into<String>) {
        self.answer = None;
        self.messages.push(ModelMessage::User(text.into()));
    }

    /// The model selector id for this session.
    pub fn model_selector(&self) -> &str {
        &self.model.id
    }

    /// The tool definitions offered to the model, whose count is used to size
    /// the model context. Exposed so context accounting can include them.
    pub fn tool_definitions(&self) -> &[ToolDefinition] {
        &self.tool_defs
    }

    /// The next turn request, or `None` when there is no pending work.
    pub fn next_request(&self) -> Option<ModelRequest> {
        if self.messages.is_empty() {
            return None;
        }
        // The system prompt is host-owned instructions, so it leads the
        // conversation as a `System` message ahead of the user history.
        let mut messages = Vec::with_capacity(self.messages.len() + 1);
        if !self.system_prompt.is_empty() {
            messages.push(ModelMessage::System(self.system_prompt.clone()));
        }
        messages.extend(self.messages.iter().cloned());
        Some(ModelRequest {
            model: self.model.model.clone(),
            messages,
            tools: self.tool_defs.clone(),
            tool_choice: ToolChoice::Auto,
            reasoning_effort: Some(self.thinking_effort),
            output_schema: None,
            max_output_tokens: None,
        })
    }

    /// Folds an assistant turn into the conversation and returns the tool
    /// intents it proposed, which the caller must execute.
    pub fn apply_assistant(&mut self, turn: ModelTurn) -> Vec<ToolIntent> {
        let tool_intents = turn.tool_intents.clone();
        self.messages.push(ModelMessage::Assistant {
            text: turn.text.clone(),
            tool_intents: turn.tool_intents,
        });
        if tool_intents.is_empty()
            && matches!(turn.finish_reason, agent_runtime::FinishReason::Stop)
            && !turn.text.trim().is_empty()
        {
            self.answer = Some(turn.text);
        } else {
            self.answer = None;
        }
        tool_intents
    }

    /// Records a status-bearing, combined bounded preview as a model message.
    pub fn record_tool_result(&mut self, call_id: &str, name: &str, outcome: &ToolOutcome) {
        let mut body = format!(
            "[process: exited={}, status={:?}, timed_out={}, cancelled={}, output_limit_exceeded={}]\n",
            outcome.exited,
            outcome.status,
            outcome.timed_out,
            outcome.cancelled,
            outcome.output_limit_exceeded,
        );
        const TRUNCATION_NOTICE: &str = "\n[output truncated; use a smaller read/search page]\n";
        let available = MAX_TOOL_RESULT_BYTES.saturating_sub(body.len() + TRUNCATION_NOTICE.len());
        let stderr_budget = available.min(outcome.stderr.len()).min(available / 2);
        let stdout = outcome.output_text(available.saturating_sub(stderr_budget));
        let stderr = outcome.error_text(stderr_budget);
        let truncated = stdout.len() < outcome.stdout.len() || stderr.len() < outcome.stderr.len();
        body.push_str(&stdout);
        if !stderr.is_empty() {
            body.push_str("\n[stderr] ");
            body.push_str(&stderr);
        }
        // Lossy UTF-8 decoding may expand bytes; bound the final encoded preview.
        let cap = MAX_TOOL_RESULT_BYTES.saturating_sub(TRUNCATION_NOTICE.len());
        let mut end = body.len().min(cap);
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        let expanded = end < body.len();
        body.truncate(end);
        if truncated || expanded || outcome.output_limit_exceeded {
            body.push_str(TRUNCATION_NOTICE);
        }
        self.messages.push(ModelMessage::ToolResult {
            call_id: call_id.to_owned(),
            name: name.to_owned(),
            content: body,
        });
    }

    /// The token size of the conversation that would next be sent to the model,
    /// plus the always-present tool definitions. Used to track context usage.
    pub fn estimate_context_tokens(&self, tool_definitions: usize) -> usize {
        let _ = tool_definitions;
        self.next_request()
            .as_ref()
            .map_or(0, crate::context::estimate_request)
    }

    /// Compacts the conversation when it crosses the manager's warm-up threshold,
    /// rolling the oldest completed turns into a summary. Returns the compaction
    /// metadata when anything was compacted, so the caller can log and display it.
    pub fn maybe_compact(
        &mut self,
        manager: &mut ContextManager,
        tool_definitions: usize,
    ) -> Option<crate::context::Compaction> {
        let context_tokens = self.estimate_context_tokens(tool_definitions);
        if !manager.should_compact(context_tokens) {
            return None;
        }
        let (condensed, compaction) = manager.compact(&self.messages, tool_definitions);
        self.messages = condensed;
        Some(compaction)
    }

    /// The current context utilization fraction against the hard limit.
    pub fn utilization(&self, manager: &ContextManager, tool_definitions: usize) -> f64 {
        let context_tokens = self.estimate_context_tokens(tool_definitions);
        manager.utilization(context_tokens)
    }

    /// How close compaction is to the hard limit, as a progress fraction.
    pub fn compaction_progress(&self, manager: &ContextManager, tool_definitions: usize) -> f64 {
        let context_tokens = self.estimate_context_tokens(tool_definitions);
        manager.compaction_progress(context_tokens)
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

/// The outcome of running a session loop.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct RunSummary {
    /// The final answer text, when the loop produced one.
    pub answer: Option<String>,
    /// The number of model turns performed.
    pub turns: u32,
    /// The number of tool calls executed.
    pub tools_executed: u32,
    /// Whether the user cancelled the loop.
    pub cancelled: bool,
    /// Whether the loop stopped after too many turns.
    pub exhausted: bool,
    /// Whether the shared wall/token budget ended the prompt.
    pub budget_exhausted: bool,
    /// A terminal failure diagnostic, if the prompt did not complete.
    pub failure: Option<String>,
}

impl RunSummary {
    /// Only an answer with no interrupted/failed disposition is successful.
    pub fn success(&self) -> bool {
        self.answer.is_some()
            && !self.cancelled
            && !self.exhausted
            && !self.budget_exhausted
            && self.failure.is_none()
    }

    /// Stable machine-readable terminal disposition.
    pub fn disposition(&self) -> &'static str {
        if self.budget_exhausted {
            "budget_exhausted"
        } else if self.cancelled {
            "cancelled"
        } else if self.failure.is_some() {
            "failed"
        } else if self.exhausted {
            "turn_limit"
        } else if self.success() {
            "completed"
        } else {
            "no_work"
        }
    }
}

/// Shared resource limits for one submitted prompt, including all retries.
#[derive(Debug, Clone, Copy)]
pub struct RunLimits {
    /// Whole-prompt wall time; executors must honor cooperative cancellation.
    pub wall_time: Duration,
    /// Conservative estimated input plus reserved output across all attempts.
    pub max_tokens: u64,
    /// Provider-enforced output tokens reserved in every context preflight.
    pub response_reserve: u32,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            wall_time: Duration::from_secs(1800),
            max_tokens: 1_000_000,
            response_reserve: 1024,
        }
    }
}

impl RunLimits {
    /// Rejects invalid limits before starting a provider or watcher.
    pub fn validate(self) -> Result<Self> {
        if self.wall_time.is_zero()
            || self.wall_time > Duration::from_secs(86_400)
            || self.max_tokens == 0
            || self.max_tokens > 1_000_000_000
            || !(1..=1_048_576).contains(&self.response_reserve)
        {
            return Err(Error::Config {
                path: None,
                reason: "run limits require positive wall time <=24h, tokens <=1000000000, and response reserve <=1048576".into(),
            });
        }
        Ok(self)
    }
}

fn validate_turn(turn: &ModelTurn) -> Result<()> {
    let valid_finish = matches!(turn.finish_reason, agent_runtime::FinishReason::Stop)
        && turn.tool_intents.is_empty()
        && !turn.text.trim().is_empty()
        || matches!(turn.finish_reason, agent_runtime::FinishReason::ToolCalls)
            && !turn.tool_intents.is_empty();
    if !valid_finish {
        return Err(Error::InvalidModelTurn {
            reason: format!(
                "incomplete or inconsistent finish `{}`",
                finish_reason_str(&turn.finish_reason)
            ),
        });
    }
    if turn.tool_intents.len() > 32 {
        return Err(Error::InvalidModelTurn {
            reason: "more than 32 tool calls in one turn".into(),
        });
    }
    let mut seen = HashSet::new();
    for intent in &turn.tool_intents {
        if intent.id.is_empty()
            || intent.id.len() > 256
            || intent.id.contains('\0')
            || intent.name.is_empty()
            || intent.name.len() > 128
            || intent.name.contains('\0')
            || !seen.insert(&intent.id)
            || !intent.arguments.is_object()
        {
            return Err(Error::InvalidModelTurn {
                reason: "invalid or duplicate tool identity/arguments".into(),
            });
        }
        let args =
            serde_json::to_vec(&intent.arguments).map_err(|error| Error::InvalidModelTurn {
                reason: format!("tool argument encoding failed: {error}"),
            })?;
        if args.len() > 1024 * 1024 {
            return Err(Error::InvalidModelTurn {
                reason: "tool arguments exceed 1 MiB".into(),
            });
        }
    }
    Ok(())
}

fn fold_outcome<S: EventSink>(
    session: &mut AgentSession,
    recorder: &mut Option<&mut dyn Recorder>,
    sink: &S,
    turn: usize,
    intent: &ToolIntent,
    outcome: &ToolOutcome,
) -> Result<()> {
    session.record_tool_result(&intent.id, &intent.name, outcome);
    if let Some(recorder) = recorder.as_mut() {
        recorder.tool_result(turn, &intent.id, &intent.name, &intent.arguments, outcome)?;
    }
    sink.send(Event::ToolResult {
        description: crate::tools::describe_tool_call(intent),
        name: intent.name.clone(),
        failed: outcome.failed(),
    })
}

struct PromptBudget {
    deadline: Instant,
    expired: Arc<AtomicBool>,
    stop: Option<mpsc::Sender<()>>,
    watcher: Option<JoinHandle<()>>,
}

impl PromptBudget {
    fn start(wall_time: Duration, cancellation: &CancellationToken) -> Result<Self> {
        let deadline = Instant::now() + wall_time;
        let expired = Arc::new(AtomicBool::new(false));
        let thread_expired = Arc::clone(&expired);
        let token = cancellation.clone();
        let (stop, rx) = mpsc::channel();
        let watcher = thread::Builder::new()
            .name("prompt-deadline".into())
            .spawn(move || {
                loop {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        thread_expired.store(true, Ordering::SeqCst);
                        token.cancel();
                        break;
                    }
                    match rx.recv_timeout(remaining.min(Duration::from_millis(25))) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if agent_runtime::take_interrupted() {
                                token.cancel();
                            }
                        }
                    }
                }
            })?;
        Ok(Self {
            deadline,
            expired,
            stop: Some(stop),
            watcher: Some(watcher),
        })
    }

    fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
}

impl Drop for PromptBudget {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(watcher) = self.watcher.take()
            && watcher.join().is_err()
        {
            tracing::error!("prompt deadline watcher failed");
        }
    }
}

/// Maps a `FinishReason` to a short, stable string for the session record.
fn finish_reason_str(reason: &agent_runtime::FinishReason) -> &str {
    match reason {
        agent_runtime::FinishReason::Stop => "stop",
        agent_runtime::FinishReason::Length => "length",
        agent_runtime::FinishReason::ToolCalls => "tool_calls",
        agent_runtime::FinishReason::ContentFilter => "content_filter",
        agent_runtime::FinishReason::Other(s) => s.as_str(),
    }
}

/// The multi-turn loop driver.
pub struct AgentRunner {
    pub max_turns: u32,
    /// How to retry a turn that ends in a transient, recoverable failure.
    pub retry: RetryPolicy,
    /// Limits shared by the complete prompt rather than reset on retries.
    pub limits: RunLimits,
}

impl Default for AgentRunner {
    fn default() -> Self {
        AgentRunner {
            max_turns: MAX_TURNS,
            retry: RetryPolicy::default(),
            limits: RunLimits::default(),
        }
    }
}

impl AgentRunner {
    /// Creates a loop driver with an explicit retry policy.
    pub fn with_retry(max_turns: u32, retry: RetryPolicy) -> Self {
        AgentRunner {
            max_turns,
            retry,
            limits: RunLimits::default(),
        }
    }

    /// Sets validated whole-prompt limits.
    pub fn with_limits(mut self, limits: RunLimits) -> Result<Self> {
        self.limits = limits.validate()?;
        Ok(self)
    }
}

impl AgentRunner {
    /// Runs the loop to completion, streaming [`Event`]s through `sink`.
    ///
    /// Each parameter is a distinct collaborator the loop needs: `session` (the
    /// conversation), `transport` (the model), `executor` (sandbox tools),
    /// `sink` (events to the UI), `cancellation` (cooperative interrupt),
    /// `context` (rolling compaction bound), and an optional `recorder` that
    /// captures the durable record (reasoning included) for later inspection.
    /// Compaction only ever trims the model context; the recorder is the
    /// durable source of truth.
    #[allow(clippy::too_many_arguments)]
    pub fn run<M, E, S>(
        &self,
        session: &mut AgentSession,
        transport: &M,
        executor: &E,
        sink: &S,
        cancellation: &CancellationToken,
        context: &mut ContextManager,
        mut recorder: Option<&mut dyn Recorder>,
    ) -> Result<RunSummary>
    where
        M: ModelTransport,
        E: ToolExecutor,
        S: EventSink,
    {
        self.limits.validate()?;
        if !(1..=MAX_TURNS).contains(&self.max_turns) {
            return Err(Error::Config {
                path: None,
                reason: format!("turn limit must be in 1..={MAX_TURNS}"),
            });
        }
        let budget = PromptBudget::start(self.limits.wall_time, cancellation)?;
        let mut summary = RunSummary::default();
        session.answer = None;
        let result = (|| {
            if let Some(recorder) = recorder.as_mut() {
                recorder.session_start()?;
            }
            self.run_inner(
                session,
                transport,
                executor,
                sink,
                cancellation,
                context,
                &mut recorder,
                &budget,
                &mut summary,
            )
        })();
        summary.budget_exhausted |= budget.expired.load(Ordering::SeqCst)
            || budget.remaining().is_zero()
            || matches!(result, Err(Error::RunBudget { .. }));
        summary.cancelled |= cancellation.is_cancelled() && !summary.budget_exhausted;
        if let Err(error) = &result {
            summary.failure = Some(error.describe());
        }
        if summary.cancelled || summary.budget_exhausted || summary.failure.is_some() {
            summary.answer = None;
            session.answer = None;
        }
        if let Some(recorder) = recorder.as_mut()
            && let Err(error) = recorder.session_finish(&summary)
        {
            return Err(Error::Recording {
                reason: match &result {
                    Ok(()) => error.describe(),
                    Err(original) => format!(
                        "{}; terminal record: {}",
                        original.describe(),
                        error.describe()
                    ),
                },
            });
        }
        result?;
        let terminal = if let Some(failure) = &summary.failure {
            Event::Failed(failure.clone())
        } else if let Some(answer) = &summary.answer {
            Event::Finished {
                message: answer.clone(),
            }
        } else {
            Event::PromptEnd {
                exhausted: summary.exhausted || summary.budget_exhausted,
                cancelled: summary.cancelled,
            }
        };
        sink.send(terminal)?;
        Ok(summary)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_inner<M: ModelTransport, E: ToolExecutor, S: EventSink>(
        &self,
        session: &mut AgentSession,
        transport: &M,
        executor: &E,
        sink: &S,
        cancellation: &CancellationToken,
        context: &mut ContextManager,
        recorder: &mut Option<&mut dyn Recorder>,
        budget: &PromptBudget,
        summary: &mut RunSummary,
    ) -> Result<()> {
        let started_at = Instant::now();
        let mut input = 0_u64;
        let mut output = 0_u64;
        let mut total = 0_u64;
        let mut tokens_remaining = self.limits.max_tokens;
        let mut actions = agent_runtime::ActionHashRing::new();
        while summary.turns < self.max_turns {
            if cancellation.is_cancelled() || budget.remaining().is_zero() {
                break;
            }
            let Some(mut request) = session.next_request() else {
                break;
            };
            if let Some(compaction) = context.prepare(&mut request, self.limits.response_reserve)?
                && compaction.compacted_turns > 0
            {
                sink.send(Event::Note(format!(
                    "context compacted: {} complete groups in a lossy, non-authoritative summary",
                    compaction.compacted_turns,
                )))?;
            }
            session.messages = request
                .messages
                .iter()
                .skip(usize::from(!session.system_prompt.is_empty()))
                .cloned()
                .collect();
            summary.turns += 1;
            sink.send(Event::TurnStart {
                model: session.model_selector().into(),
            })?;
            let turn_value = match self.drive_turn(
                &request,
                transport,
                sink,
                cancellation,
                &LiveProgress {
                    session,
                    context,
                    tool_defs: session.tool_defs.len(),
                    started_at,
                    cumulative_total: input.saturating_add(output),
                },
                budget,
                &mut tokens_remaining,
                recorder,
            ) {
                Ok(value) => value,
                Err(error) => {
                    if matches!(error, Error::RunBudget { .. }) {
                        summary.budget_exhausted = true;
                    } else if !cancellation.is_cancelled() {
                        summary.failure = Some(error.describe());
                    }
                    break;
                }
            };
            let turn_index = match recorder.as_mut() {
                Some(recorder) => {
                    let index = recorder.turn_start(&turn_value)?;
                    recorder.turn_finish(
                        index,
                        &turn_value.usage,
                        finish_reason_str(&turn_value.finish_reason),
                    )?;
                    index
                }
                None => summary.turns as usize,
            };
            if let Some(usage) = turn_value.usage {
                input = input.saturating_add(usage.input_tokens);
                output = output.saturating_add(usage.output_tokens);
                total = total.saturating_add(usage.total_tokens);
            }
            if let Err(error) = validate_turn(&turn_value) {
                summary.failure = Some(error.describe());
                break;
            }
            let intents = session.apply_assistant(turn_value);
            let terminal = intents.is_empty();
            for (index, intent) in intents.iter().enumerate() {
                if cancellation.is_cancelled() || summary.failure.is_some() {
                    for remaining in &intents[index..] {
                        fold_outcome(
                            session,
                            recorder,
                            sink,
                            turn_index,
                            remaining,
                            &ToolOutcome::rejected_with("not executed: prompt interrupted"),
                        )?;
                    }
                    break;
                }
                let hash = agent_runtime::compute_action_hash(&intent.name, &intent.arguments);
                let decision = actions.check_proposed_action(&hash);
                if decision.is_loop_break() {
                    let reason = "Repeated identical tool arguments are blocked. Inspect the current state or choose a different action; filesystem change was not observed by this detector.";
                    fold_outcome(
                        session,
                        recorder,
                        sink,
                        turn_index,
                        intent,
                        &ToolOutcome::rejected_with(reason),
                    )?;
                    sink.send(Event::Note(reason.into()))?;
                    if matches!(decision, agent_runtime::LoopDecision::CircuitBreaker { .. }) {
                        summary.failure = Some(
                            "Repeated-action circuit breaker: too many identical tool proposals"
                                .into(),
                        );
                    }
                    continue;
                }
                if let Some(recorder) = recorder.as_mut() {
                    recorder.tool_dispatch(turn_index, intent)?;
                }
                let outcome = match executor.execute(intent, cancellation) {
                    Ok(outcome) => {
                        summary.tools_executed += 1;
                        actions.record_action(hash, intent.name.clone());
                        outcome
                    }
                    Err(error) => {
                        if matches!(error, Error::ToolPolicy { .. } | Error::ToolRender { .. }) {
                            sink.send(Event::Note(error.describe()))?;
                        } else {
                            summary.failure = Some(error.describe());
                        }
                        ToolOutcome::rejected_with(error.describe())
                    }
                };
                if outcome.cancelled {
                    cancellation.cancel();
                }
                fold_outcome(session, recorder, sink, turn_index, intent, &outcome)?;
            }
            self.emit_progress(sink, session, context, input, output, total, started_at)?;
            if terminal || summary.failure.is_some() || cancellation.is_cancelled() {
                break;
            }
        }
        summary.answer = session.answer.clone();
        summary.exhausted = summary.turns >= self.max_turns
            && summary.answer.is_none()
            && summary.failure.is_none()
            && !cancellation.is_cancelled();
        Ok(())
    }

    /// Streams one turn, retrying transient failures with bounded backoff.
    ///
    /// The loop reuses the same [`ModelRequest`] (rebuildable from the session,
    /// which excludes any partial output of a failed attempt) across attempts so
    /// a retry replays the turn from a clean state. Each attempt is a fresh
    /// transport call, so it also gets a fresh per-turn deadline: a turn that
    /// merely ran past its deadline once can finish on the next attempt.
    ///
    ///
    /// While a turn streams, a live progress update is emitted at a bounded rate
    /// so the stats bar tracks working speed, context, and elapsed time as the
    /// model generates, not only after the turn ends.
    ///
    /// A failure is retried only when [`agent_runtime::Error::is_retryable`]
    /// reports it as a temporal, recoverable condition (network drop, provider
    /// timeout, transient server error). Cancellation is never retried, and once
    /// the policy's attempt budget is spent the final error is returned so the
    /// caller reports it. Between attempts an [`Event::Note`] is emitted so the
    /// user sees the retry is in progress; the best-effort sink send only means
    /// the UI is gone.
    #[allow(clippy::too_many_arguments)]
    fn drive_turn<M, S>(
        &self,
        request: &ModelRequest,
        transport: &M,
        sink: &S,
        cancellation: &CancellationToken,
        progress: &LiveProgress,
        budget: &PromptBudget,
        tokens_remaining: &mut u64,
        recorder: &mut Option<&mut dyn Recorder>,
    ) -> Result<ModelTurn>
    where
        M: ModelTransport,
        S: EventSink,
    {
        // Each retryable attempt is granted a larger budget than the last so a
        // turn that merely ran past one deadline can finish once an attempt has
        // room for the whole generation. The budget grows linearly with the
        // attempt number and is capped at `base × max_attempts`, so a recoverable
        // timeout can never turn a single transport call into an unbounded wait;
        // the first attempt uses the transport's configured deadline unchanged.
        let base = transport.deadline();
        let mut attempt = 1u32;
        loop {
            if cancellation.is_cancelled() {
                return Err(agent_runtime::Error::ModelTransportCancelled.into());
            }
            if budget.remaining().is_zero() {
                return Err(Error::RunBudget {
                    reason: "wall time".into(),
                });
            }
            let charge = (crate::context::estimate_request(request) as u64)
                .saturating_add(u64::from(self.limits.response_reserve));
            if charge > *tokens_remaining {
                return Err(Error::RunBudget {
                    reason: "estimated input/output tokens".into(),
                });
            }
            *tokens_remaining -= charge;
            let attempt_deadline = base
                .saturating_mul(attempt)
                .min(base.saturating_mul(self.retry.max_attempts))
                .min(budget.remaining());
            sink.send(Event::AttemptStart { attempt })?;
            if let Some(recorder) = recorder.as_mut() {
                recorder.request(request, attempt)?;
            }
            // Running output estimate and last live-update time for this attempt,
            // so the stats bar refreshes at a bounded rate while the model streams
            // rather than only after the turn completes.
            let mut attempt_output_chars: u64 = 0;
            let mut last_emit = Instant::now();
            let mut sink_error = None;
            let result = transport.stream_with_deadline(
                request,
                cancellation,
                &mut |event| {
                    if cancellation.is_cancelled() {
                        return Err(agent_runtime::Error::ModelTransportCancelled);
                    }
                    let emitted = match event {
                        ModelStreamEvent::TextDelta(delta) => {
                            attempt_output_chars =
                                attempt_output_chars.saturating_add(delta.chars().count() as u64);
                            sink.send(Event::Text(delta))
                        }
                        ModelStreamEvent::ReasoningDelta(delta) => {
                            attempt_output_chars =
                                attempt_output_chars.saturating_add(delta.chars().count() as u64);
                            sink.send(Event::Reasoning(delta))
                        }
                        ModelStreamEvent::ToolIntent(intent) => {
                            let description = crate::tools::describe_tool_call(&intent);
                            sink.send(Event::ToolCall {
                                description,
                                name: intent.name.clone(),
                            })
                        }
                    };
                    if let Err(error) = emitted {
                        sink_error = Some(error);
                        cancellation.cancel();
                        return Err(agent_runtime::Error::ModelTransportCancelled);
                    }
                    // Refresh the live stats bar at a bounded rate so the user
                    // sees the turn making progress while the model streams.
                    let now = Instant::now();
                    if now.saturating_duration_since(last_emit) >= LIVE_PROGRESS_INTERVAL {
                        last_emit = now;
                        if let Err(error) =
                            emit_live_progress(sink, progress, attempt_output_chars, now)
                        {
                            sink_error = Some(error);
                            cancellation.cancel();
                            return Err(agent_runtime::Error::ModelTransportCancelled);
                        }
                    }
                    Ok(())
                },
                attempt_deadline,
            );
            if let Some(error) = sink_error {
                return Err(error);
            }
            match result {
                Ok(turn) => return Ok(turn),
                // Retry only temporal, recoverable failures, and only while the
                // attempt budget remains and the turn was not cancelled.
                Err(error) if error.is_retryable() => {
                    if attempt >= self.retry.max_attempts || cancellation.is_cancelled() {
                        return Err(error.into());
                    }
                    let next = attempt + 1;
                    let delay = self.retry.backoff_delay(next).min(budget.remaining());
                    let delay_secs = delay.as_secs_f64();
                    let next_budget_secs = base
                        .saturating_mul(next)
                        .min(budget.remaining())
                        .as_secs_f64();
                    // Show the turn is recovering and that a longer budget is
                    // being granted, not just another identical try.
                    sink.send(Event::Note(format!(
                        "model request failed ({error}); retrying {next}/{max} in {delay_secs:.1}s with at most {next_budget_secs:.0}s; earlier streamed text is provisional",
                        max = self.retry.max_attempts,
                    )))?;
                    attempt = next;
                    let wake_at = Instant::now() + delay;
                    while Instant::now() < wake_at {
                        if cancellation.is_cancelled() {
                            return Err(agent_runtime::Error::ModelTransportCancelled.into());
                        }
                        if budget.remaining().is_zero() {
                            return Err(Error::RunBudget {
                                reason: "wall time during retry".into(),
                            });
                        }
                        thread::sleep(
                            wake_at
                                .saturating_duration_since(Instant::now())
                                .min(Duration::from_millis(25)),
                        );
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Emits a single accounting event so the UI can update the speed stat, the
    /// context bargraph, and the compaction progress bar. A closed sink stops work.
    #[allow(clippy::too_many_arguments)]
    fn emit_progress<S: EventSink>(
        &self,
        sink: &S,
        session: &AgentSession,
        context: &ContextManager,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
        started_at: Instant,
    ) -> Result<()> {
        let context_tokens = session.estimate_context_tokens(session.tool_definitions().len());
        let elapsed = started_at.elapsed().as_secs_f64().max(1e-9);
        let tokens_per_sec = total_tokens as f64 / elapsed;
        sink.send(Event::Progress {
            input_tokens,
            output_tokens,
            context_tokens,
            context_limit: context.limit_tokens(),
            context_utilization: session.utilization(context, session.tool_definitions().len()),
            compaction_progress: session
                .compaction_progress(context, session.tool_definitions().len()),
            tokens_per_sec,
            total_tokens,
            elapsed_secs: elapsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::{FinishReason, LocalModelProvider};

    #[test]
    fn assistant_helper_only_accepts_normal_nonblank_final_answers() {
        let model = Model {
            id: "test".into(),
            provider: crate::config::ModelProvider::LlamaServer,
            base_url: "http://127.0.0.1:1".into(),
            model: "test".into(),
            deadline_secs: 30,
            max_attempts: 1,
            retry_base_delay_secs: 1,
            retry_max_delay_secs: 1,
            cadence_timeout_secs: 0,
        };
        for (reason, text, expected) in [
            (FinishReason::Length, "partial", None),
            (FinishReason::ContentFilter, "filtered", None),
            (FinishReason::ToolCalls, "inconsistent", None),
            (FinishReason::Other("unknown".into()), "unknown", None),
            (FinishReason::Stop, "  ", None),
            (FinishReason::Stop, "done", Some("done")),
        ] {
            let mut session =
                AgentSession::new(model.clone(), ReasoningEffort::None, vec![], String::new());
            session.apply_assistant(ModelTurn {
                text: text.into(),
                reasoning: None,
                tool_intents: vec![],
                finish_reason: reason,
                provider: LocalModelProvider::LlamaServer,
                model: "test".into(),
                response_id: None,
                provider_request_id: None,
                usage: None,
            });
            assert_eq!(session.answer.as_deref(), expected);
        }
    }
}
