//! The transport-agnostic agent session and multi-turn loop.
//!
//! [`AgentSession`] owns the ordered conversation and builds turns.
//! [`AgentRunner::run`] drives the loop against any [`ModelTransport`], executes
//! the tool intents via a [`ToolExecutor`], and forwards progress through an
//! [`EventSink`]. Nothing here performs blocking subprocess I/O directly; the
//! executor is injected so the loop is unit-testable with fakes.

use std::time::Instant;

use agent_runtime::{
    CancellationToken, ModelMessage, ModelRequest, ModelStreamEvent, ModelTransport, ModelTurn,
    ModelUsage, ReasoningEffort, ToolChoice, ToolDefinition, ToolIntent,
};

use crate::config::Model;
use crate::context::ContextManager;
use crate::error::Result;
use crate::sandbox::ToolOutcome;

/// The maximum number of model turns in one session before the loop stops.
pub const MAX_TURNS: u32 = 50;
/// The maximum bytes of a tool result folded back to the model.
pub const MAX_TOOL_RESULT_BYTES: usize = 64 * 1024;

/// A progress event emitted while a session runs.
#[derive(Debug, Clone)]
pub enum Event {
    /// A model turn began.
    TurnStart { model: String },
    /// A fragment of model reasoning text.
    Reasoning(String),
    /// A fragment of model answer text.
    Text(String),
    /// A tool call was proposed by the model.
    ToolCall { name: String },
    /// A tool call finished; `failed` marks a non-zero or non-exiting result.
    ToolResult { name: String, failed: bool },
    /// The session produced a final answer.
    Finished { message: String },
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
        if tool_intents.is_empty() {
            self.answer = Some(turn.text);
        }
        tool_intents
    }

    /// Records one tool result, redacted and bounded, as a model message.
    pub fn record_tool_result(&mut self, call_id: &str, name: &str, outcome: &ToolOutcome) {
        let mut body = outcome.output_text(MAX_TOOL_RESULT_BYTES);
        let stderr = outcome.error_text(MAX_TOOL_RESULT_BYTES);
        if !stderr.is_empty() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&format!("[stderr] {stderr}"));
        }
        if outcome.timed_out {
            body.push_str("\n[sandbox: wall-clock timeout]");
        }
        if outcome.output_limit_exceeded {
            body.push_str(&format!(
                "\n[sandbox: output limited to {} bytes]",
                MAX_TOOL_RESULT_BYTES
            ));
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
        crate::context::estimate_messages(&self.messages, tool_definitions)
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
pub trait ToolExecutor: Send {
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
    fn session_start(&mut self);
    /// Begins one turn, capturing its reasoning trace. Returns the turn index
    /// used by later tool records.
    fn turn_start(&mut self, turn: &ModelTurn) -> usize;
    /// Completes one turn, folding in provider token usage.
    fn turn_finish(&mut self, turn: usize, usage: &Option<ModelUsage>, finish_reason: &str);
    /// Records an approved tool call and its sandbox outcome.
    fn tool_result(
        &mut self,
        turn: usize,
        call_id: &str,
        tool: &str,
        args: &serde_json::Value,
        outcome: &ToolOutcome,
    );
    /// Writes the terminal session-finish event with session totals.
    fn session_finish(&mut self, success: bool);
}

/// The outcome of running a session loop.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
}

impl Default for AgentRunner {
    fn default() -> Self {
        AgentRunner {
            max_turns: MAX_TURNS,
        }
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
        let tool_definitions = session.tool_definitions().len();
        let mut summary = RunSummary::default();
        let mut turn = 0u32;
        let started_at = Instant::now();
        let mut cumulative_tokens: u64 = 0;
        let mut cumulative_input: u64 = 0;
        let mut cumulative_output: u64 = 0;

        // Record the durable session boundary before any turn, so the record
        // has a clean start (and a matching finish) even for an empty run.
        if let Some(recorder) = recorder.as_mut() {
            recorder.session_start();
        }

        while turn < self.max_turns {
            let Some(request) = session.next_request() else {
                break;
            };
            turn += 1;

            sink.send(Event::TurnStart {
                model: session.model_selector().to_owned(),
            })?;

            // Stream the turn, forwarding text/reasoning to the UI and recording
            // each tool intent the model proposes so the UI can show it live.
            // The callback must return `agent_runtime::Result`, so forwarding to
            // the sink is best-effort: a closed channel only means the UI is gone,
            // which the loop surfaces on the next turn's own `sink.send`.
            let result = transport.stream(&request, cancellation, &mut |event| match event {
                ModelStreamEvent::TextDelta(delta) => {
                    if !delta.is_empty() {
                        let _ = sink.send(Event::Text(delta));
                    }
                    Ok(())
                }
                ModelStreamEvent::ReasoningDelta(delta) => {
                    if !delta.is_empty() {
                        let _ = sink.send(Event::Reasoning(delta));
                    }
                    Ok(())
                }
                ModelStreamEvent::ToolIntent(intent) => {
                    let _ = sink.send(Event::ToolCall {
                        name: intent.name.clone(),
                    });
                    Ok(())
                }
            });

            let turn_value = match result {
                Ok(turn) => turn,
                Err(error) => {
                    sink.send(Event::Failed(error.to_string()))?;
                    summary.cancelled = cancellation.is_cancelled();
                    return Ok(summary);
                }
            };

            // Capture the full record first: reasoning + finish + usage. This is
            // the durable copy that compaction and the UI may later drop.
            let turn_index = recorder.as_mut().map(|rec| rec.turn_start(&turn_value));
            let finish_reason = finish_reason_str(&turn_value.finish_reason);
            if let Some(recorder) = recorder.as_mut() {
                recorder.turn_finish(turn_index.unwrap_or(0), &turn_value.usage, finish_reason);
            }
            if let Some(usage) = &turn_value.usage {
                cumulative_tokens += usage.total_tokens;
                cumulative_input += usage.input_tokens;
                cumulative_output += usage.output_tokens;
            }

            // No tools proposed: this assistant text is the final answer.
            let tool_intents = session.apply_assistant(turn_value);
            let terminal = tool_intents.is_empty();

            if !terminal {
                for intent in tool_intents {
                    if cancellation.is_cancelled() {
                        summary.cancelled = true;
                        return Ok(summary);
                    }
                    match executor.execute(&intent, cancellation) {
                        Ok(outcome) => {
                            summary.tools_executed += 1;
                            session.record_tool_result(&intent.id, &intent.name, &outcome);
                            if let Some(recorder) = recorder.as_mut() {
                                recorder.tool_result(
                                    turn_index.unwrap_or(0),
                                    &intent.id,
                                    &intent.name,
                                    &intent.arguments,
                                    &outcome,
                                );
                            }
                            let _ = sink.send(Event::ToolResult {
                                name: intent.name.clone(),
                                failed: outcome.failed(),
                            });
                        }
                        Err(error) => {
                            // A rejected or failed tool is reported, not fatal.
                            if let Some(recorder) = recorder.as_mut() {
                                recorder.tool_result(
                                    turn_index.unwrap_or(0),
                                    &intent.id,
                                    &intent.name,
                                    &intent.arguments,
                                    &crate::sandbox::ToolOutcome::rejected(),
                                );
                            }
                            let _ = sink.send(Event::ToolResult {
                                name: intent.name.clone(),
                                failed: true,
                            });
                            let _ = sink.send(Event::Failed(error.describe()));
                        }
                    }

                    // Compact the model context now that this turn is fully
                    // recorded, so the next request stays within the model's
                    // window. The record is untouched.
                    if let Some(compaction) = session.maybe_compact(context, tool_definitions)
                        && compaction.compacted_turns > 0
                    {
                        let _ = sink.send(Event::Note(format!(
                        "context compacted: {} earlier turn(s) rolled into a summary (kept in the session log)",
                        compaction.compacted_turns
                    )));
                    }
                }
            }

            // Report accounting after every turn — including the final one,
            // whose report doubles as the session's last — so the live stats
            // bar (speed, context utilization, compaction progress, and the
            // compaction ETA) tracks the session while it runs, not only when
            // it ends.
            self.emit_progress(
                sink,
                session,
                context,
                cumulative_input,
                cumulative_output,
                cumulative_tokens,
                started_at,
            )?;
            if terminal {
                break;
            }
        }

        if turn >= self.max_turns {
            summary.exhausted = true;
        }
        summary.turns = turn;
        summary.answer = session.answer.clone();
        if let Some(answer) = &summary.answer {
            let _ = sink.send(Event::Finished {
                message: answer.clone(),
            });
        }
        // Close the durable session record. A run that was cancelled or
        // exhausted after too many turns is recorded as unsuccessful; a normal
        // completion (with or without a final answer) is recorded as success.
        let success = !summary.cancelled && !summary.exhausted;
        if let Some(recorder) = recorder.as_mut() {
            recorder.session_finish(success);
        }
        Ok(summary)
    }

    /// Emits a single accounting event so the UI can update the speed stat, the
    /// context bargraph, and the compaction progress bar. Non-fatal: a dropped
    /// sink only means the UI is gone.
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
