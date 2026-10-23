//! The transport-agnostic agent session and multi-turn loop.
//!
//! [`AgentSession`] owns the ordered conversation and builds turns.
//! [`super::NativeRuntime::run`] drives the loop against any [`ModelTransport`], executes
//! the tool intents via a [`ToolExecutor`], and forwards progress through an
//! [`EventSink`]. Nothing here performs blocking subprocess I/O directly; the
//! executor is injected so the loop is unit-testable with fakes.
//!
//! Steering: prompts submitted during an active task are delivered as steering
//! messages at the next safe interruption point (during generation, backoff
//! waits, or between tool calls). Steering preserves the original task context
//! and does not create a new session.

use std::collections::HashSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{
    CancellationToken, ModelMessage, ModelRequest, ModelTurn, ModelUsage, ReasoningEffort,
    ToolChoice, ToolDefinition, ToolIntent,
};

/// Host-selected immutable model identity; contains no policy or credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelIdentity {
    /// Host-facing selector.
    pub id: String,
    /// Provider-facing model identity.
    pub model: String,
}
use super::context::ContextManager;
use super::error::{Error, Result};

use super::ToolOutcome;

/// The maximum number of model turns in one session before the loop stops.
///
/// Large enough that a long task runs uninterrupted to a result: the loop keeps
/// turning while the model has not answered, and stops only on an answer, a
/// detected loop/hang (repeated-action breaker, cadence watchdog), a real
/// failure, or one of the prompt budgets. Five hundred turns is a safety bound,
/// not an expected duration.
pub const MAX_TURNS: u32 = 500;
/// The maximum bytes of a tool result folded back to the model.
pub const MAX_TOOL_RESULT_BYTES: usize = 8 * 1024;

/// Minimum gap between live progress updates emitted while a turn streams, so
/// the stats bar tracks progress without flooding the sink on every token.
pub(super) const LIVE_PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Read-only, turn-scoped state the streaming loop uses to emit live progress.
///
/// The provider's token usage is not known until the turn ends, so the running
/// output estimate is derived from the characters streamed so far; the context
/// figures come from the session's live context, which does not change mid-turn.
/// `cumulative_output` carries output tokens from earlier turns so the live
/// path can report the whole-session average on the same basis as the post-turn
/// figure (cumulative output over decode time).
pub(super) struct LiveProgress<'a> {
    pub(super) session: &'a AgentSession,
    pub(super) context: &'a ContextManager,
    pub(super) tool_defs: usize,
    pub(super) started_at: Instant,
    pub(super) cumulative_input: u64,
    pub(super) cumulative_total: u64,
    pub(super) cumulative_output: u64,
    pub(super) decode_seconds: f64,
}

/// Emits a single live progress update while a turn streams, so the stats bar
/// shows working speed, context, and elapsed time as the model generates rather
/// than only after the turn completes. Best-effort: a closed sink only means the
/// UI is gone. The running output is estimated from streamed characters because
/// the provider reports usage only when the turn ends.
///
/// Output speed uses the observed decode interval, excluding first-token wait
/// and event handling. Provider-internal generation timing is not available.
pub(super) fn emit_live_progress<S: EventSink>(
    sink: &S,
    progress: &LiveProgress,
    output_chars: u64,
    generation_secs: f64,
    prior_attempt_secs: f64,
    at: Instant,
) -> Result<()> {
    let output_tokens = output_chars.div_ceil(4);
    let elapsed = at
        .saturating_duration_since(progress.started_at)
        .as_secs_f64()
        .max(1e-9);
    let total_tokens = progress.cumulative_total.saturating_add(output_tokens);
    // Whole-session average over decode time: output tokens from earlier
    // turns plus this turn's estimate, over the decode seconds from earlier
    // turns plus this turn's, on the same basis as the post-turn average.
    let cumulative_output = progress.cumulative_output.saturating_add(output_tokens);
    let cumulative_decode = progress.decode_seconds + prior_attempt_secs + generation_secs;
    sink.send(Event::Progress {
        token_accounting: TokenAccounting::Estimated,
        input_tokens: progress.cumulative_input,
        output_tokens: cumulative_output,
        context_tokens: progress.session.estimate_context_tokens(progress.tool_defs),
        context_limit: progress.context.limit_tokens(),
        context_utilization: progress
            .session
            .utilization(progress.context, progress.tool_defs),
        compaction_progress: progress
            .session
            .compaction_progress(progress.context, progress.tool_defs),
        generation_tokens_per_sec: token_rate(output_tokens, generation_secs),
        average_tokens_per_sec: token_rate(cumulative_output, cumulative_decode),
        total_tokens,
        elapsed_secs: elapsed,
    })
}

pub(super) fn token_rate(tokens: u64, seconds: f64) -> f64 {
    if seconds > 0.0 {
        tokens as f64 / seconds
    } else {
        0.0
    }
}

/// Measures only intervals between decoded events, not request/terminal waits
/// or host work inside callbacks. A single buffered event has no measured rate.
#[derive(Default)]
pub(super) struct DecodeClock {
    first_event: Option<Instant>,
    handling: Duration,
    seconds: f64,
}

impl DecodeClock {
    pub(super) fn observe(&mut self, at: Instant) {
        let start = self.first_event.get_or_insert(at);
        self.seconds = at
            .saturating_duration_since(*start)
            .saturating_sub(self.handling)
            .as_secs_f64();
    }

    pub(super) fn exclude_handling(&mut self, duration: Duration) {
        self.handling = self.handling.saturating_add(duration);
    }

    pub(super) fn seconds(&self) -> f64 {
        self.seconds
    }
}

/// Provenance of reported token counts, separate from context estimates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenAccounting {
    /// Complete provider usage for accepted turns.
    Provider,
    /// Provisional streamed-output estimate, not provider usage.
    Estimated,
    #[default]
    /// One or more accepted turns did not report provider usage.
    Unavailable,
}

/// A progress event emitted while a session runs.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Correlated steering state. Claimed means the runtime is interrupting.
    Control(super::GuidanceNotice),
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
        /// Provider-reported, running estimate, or unavailable usage.
        token_accounting: TokenAccounting,
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
        /// Output tokens per observed decode second (first-token wait,
        /// event handling, and tool execution excluded). While a
        /// turn streams this is derived from the streamed output estimate;
        /// afterwards it is cumulative accepted output over observed decode
        /// time, including discarded attempts.
        generation_tokens_per_sec: f64,
        /// Average tokens per second over decode time: cumulative output
        /// tokens divided by the same generation seconds that drive `t/s`, so
        /// the average is on the same basis and is directly comparable to the
        /// live speed (prompt tokens and inter-turn gaps are excluded).
        average_tokens_per_sec: f64,
        /// Cumulative provider tokens observed across the session.
        total_tokens: u64,
        /// Wall-clock seconds since the run started.
        elapsed_secs: f64,
    },
}

/// The sink that receives [`Event`]s, typically a channel to the UI.
pub trait EventSink {
    /// Forwards one event; an error (e.g. a closed channel) stops the loop.
    fn send(&self, event: Event) -> Result<()>;
}

/// The transport-agnostic conversation state.
pub struct AgentSession {
    pub(super) model: ModelIdentity,
    pub(super) thinking_effort: ReasoningEffort,
    pub(super) system_prompt: String,
    pub(super) messages: Vec<ModelMessage>,
    pub(super) tool_defs: Vec<ToolDefinition>,
    pub(super) answer: Option<String>,
}

impl AgentSession {
    /// Creates an empty session for one model.
    pub fn new(
        model: ModelIdentity,
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

    /// The conversation messages. Exposed for context inspection and testing.
    pub fn messages(&self) -> &[ModelMessage] {
        &self.messages
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
            tool_choice: if self.tool_defs.is_empty() {
                ToolChoice::None
            } else {
                ToolChoice::Auto
            },
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
            && matches!(turn.finish_reason, crate::FinishReason::Stop)
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
        let stdout = model_output(&outcome.stdout, available.saturating_sub(stderr_budget));
        let stderr = model_output(&outcome.stderr, stderr_budget);
        let truncated = stdout.len() < outcome.stdout.len() || stderr.len() < outcome.stderr.len();
        body.push_str(&stdout);
        if !stderr.is_empty() {
            body.push_str("\n[stderr] ");
            body.push_str(&stderr);
        }
        // Lossy UTF-8 decoding may expand bytes; bound the final encoded preview.
        let cap = MAX_TOOL_RESULT_BYTES.saturating_sub(TRUNCATION_NOTICE.len() + 2);
        let mut encoded = 2usize;
        let mut end = 0;
        for (index, character) in body.char_indices() {
            let cost = match character {
                '"' | '\\' => 2,
                '\n' | '\r' | '\t' => 2,
                character if character.is_control() && character <= '\u{1f}' => 6,
                _ => character.len_utf8(),
            };
            if encoded.saturating_add(cost) > cap {
                break;
            }
            encoded += cost;
            end = index + character.len_utf8();
        }
        let expanded = end < body.len();
        body.truncate(end);
        if truncated || expanded || outcome.output_limit_exceeded {
            body.push_str(TRUNCATION_NOTICE);
        }

        fn model_output(bytes: &[u8], limit: usize) -> String {
            if limit == 0 {
                return String::new();
            }
            if bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
                use sha2::{Digest, Sha256};
                return format!(
                    "[binary output omitted: {} bytes; sha256:{}]",
                    bytes.len(),
                    hex::encode(Sha256::digest(bytes))
                );
            }
            let mut end = bytes.len().min(limit);
            while std::str::from_utf8(&bytes[..end]).is_err() {
                end -= 1;
            }
            String::from_utf8_lossy(&bytes[..end]).into_owned()
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
            .map_or(0, super::context::estimate_request)
    }

    /// Compacts the conversation when it crosses the manager's warm-up threshold,
    /// rolling the oldest completed turns into a summary. Returns the compaction
    /// metadata when anything was compacted, so the caller can log and display it.
    pub fn maybe_compact(
        &mut self,
        manager: &mut ContextManager,
        tool_definitions: usize,
    ) -> Option<super::context::Compaction> {
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
    /// Host-facing description, without granting authority.
    fn describe(&self, intent: &ToolIntent) -> String {
        intent.name.clone()
    }
    /// Host-declared observations may repeat without the effect repeat breaker.
    /// This is not a retry permission: the runtime never replays a tool itself.
    fn repeatable(&self, _intent: &ToolIntent) -> bool {
        false
    }
    /// Optional host guidance when identical observation arguments recur.
    fn repeated_observation_notice(&self, _intent: &ToolIntent) -> Option<String> {
        None
    }
    /// Renders and executes one tool intent inside the sandbox.
    fn execute(&self, intent: &ToolIntent, cancellation: &CancellationToken)
    -> Result<ToolOutcome>;
}

/// A durable, pluggable sink for the session record. The production
/// implementation supplied by the host writes a structured
/// journal and a readable transcript; tests can record in memory. Recording the
/// full reasoning trace here is what keeps thinking inspectable even after
/// compaction removes it from the model context.
pub trait Recorder: Send {
    /// Persists a provisional decoded provider event before presentation.
    /// Failure stops the loop before a complete proposal can cause effects.
    fn model_event(&mut self, _event: &crate::ModelStreamEvent) -> Result<()> {
        Ok(())
    }
    /// Persists a correlated control state before delivery or further effects.
    fn control(&mut self, notice: &super::GuidanceNotice) -> Result<()> {
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
        // The defaults are safety bounds, not expected runtimes: a prompt runs
        // uninterrupted until it reaches a result, is detected as hung or
        // looping, or genuinely fails. Operators bound individual runs with
        // --max-run-secs / --max-run-tokens.
        Self {
            wall_time: Duration::from_secs(86_400),
            max_tokens: 100_000_000,
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

/// Validates complete finish classification and bounded unique tool proposals.
pub fn validate_turn(turn: &ModelTurn) -> Result<()> {
    let valid_finish = matches!(turn.finish_reason, crate::FinishReason::Stop)
        && turn.tool_intents.is_empty()
        && !turn.text.trim().is_empty()
        || matches!(turn.finish_reason, crate::FinishReason::ToolCalls)
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

pub(super) fn record_notice<S: EventSink>(
    sink: &S,
    recorder: &mut Option<&mut dyn Recorder>,
    message: String,
) -> Result<()> {
    if let Some(recorder) = recorder.as_mut() {
        recorder.notice(&message)?;
    }
    sink.send(Event::Note(message))
}

pub(super) fn fold_outcome<S: EventSink, E: ToolExecutor>(
    session: &mut AgentSession,
    recorder: &mut Option<&mut dyn Recorder>,
    sink: &S,
    executor: &E,
    turn: usize,
    intent: &ToolIntent,
    outcome: &ToolOutcome,
) -> Result<()> {
    session.record_tool_result(&intent.id, &intent.name, outcome);
    if let Some(recorder) = recorder.as_mut() {
        recorder.tool_result(turn, &intent.id, &intent.name, &intent.arguments, outcome)?;
    }
    sink.send(Event::ToolResult {
        description: executor.describe(intent),
        name: intent.name.clone(),
        failed: outcome.failed(),
    })
}

pub(super) struct PromptBudget {
    pub(super) started_at: Instant,
    pub(super) deadline: Instant,
    pub(super) expired: Arc<AtomicBool>,
    stop: Option<mpsc::Sender<()>>,
    watcher: Option<JoinHandle<()>>,
}

impl PromptBudget {
    pub(super) fn start(wall_time: Duration, cancellation: &CancellationToken) -> Result<Self> {
        let started_at = Instant::now();
        let deadline = started_at + wall_time;
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
                            if crate::take_interrupted() {
                                token.cancel();
                            }
                        }
                    }
                }
            })?;
        Ok(Self {
            started_at,
            deadline,
            expired,
            stop: Some(stop),
            watcher: Some(watcher),
        })
    }

    pub(super) fn remaining(&self) -> Duration {
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
pub(super) fn finish_reason_str(reason: &crate::FinishReason) -> &str {
    match reason {
        crate::FinishReason::Stop => "stop",
        crate::FinishReason::Length => "length",
        crate::FinishReason::ToolCalls => "tool_calls",
        crate::FinishReason::ContentFilter => "content_filter",
        crate::FinishReason::Other(s) => s.as_str(),
    }
}
