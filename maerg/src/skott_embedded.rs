//! Embedded Skott agent session for Kvist.
//!
//! Kvist embeds Skott as a library and drives its agent loop through
//! [`skott::start_session`]. The session worker runs on its own thread and
//! streams [`Event`]s over a bounded channel that Kvist pumps on its UI
//! thread. Kvist renders two views of the same live session:
//!
//! - The **Skott view** ([`ActivePane::Skott`]): a detailed, live progress
//!   feed of every event — turns, attempts, tool calls, results, reasoning,
//!   and notices.
//! - The **Kvist view** ([`ActivePane::Kvist`]): a compact metrics summary
//!   plus the final result.
//!
//! The user switches views with Ctrl+PageUp (Kvist) / Ctrl+PageDown (Skott),
//! or Alt+Left / Alt+Right while a session is active. Skott remains a
//! standalone application; Kvist depends on it for all agent interactions.
//!
//! Kvist's completion policy ([`continuation_decision`]) refuses to treat a
//! run as complete until the agent is finished, blocked for a good reason, or
//! awaiting a human decision. Any other stop — the model returning control
//! without finishing, or a transient failure — is a continuation: Kvist
//! resubmits a focused prompt through Skott to ensure the task keeps making
//! progress.

use std::sync::mpsc;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sav::ReasoningEffort;

use skott::{
    PromptSender, SessionHandle,
    config::{Config, Model},
    context::ContextManager,
    executor::SandboxExecutor,
    session::{AgentSession, Event, Recorder, RunLimits, StopReason},
    start_session, system_prompt,
    toolchain::HostProbe,
    tools::ToolRegistry,
};

use crate::{KvistError, Result};

/// The active view in the split-pane layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivePane {
    /// The Kvist shell: compact metrics and the final result.
    #[default]
    Kvist,
    /// The Skott agent view: the detailed, live progress feed.
    Skott,
    /// Both views side by side (when the terminal is wide enough).
    Both,
}

/// Kvist's completion policy for one agent run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationDecision {
    /// The agent finished the task; Kvist may mark it completed.
    Finished,
    /// The agent is blocked for a good reason or needs a human decision; Kvist
    /// may mark it blocked or awaiting-decision.
    Blocked,
    /// The agent returned control without finishing and without a legitimate
    /// block; Kvist resubmits a continuation prompt through Skott.
    Continue,
}

/// Maps a Skott [`StopReason`] to Kvist's completion policy.
///
/// Only three terminal states are acceptable: the task is finished
/// ([`StopReason::Completed`]), the agent is blocked for a good reason or
/// awaiting a decision (cancellation, a budget limit, a stall, or exhausted
/// recovery guidance), or — for every other stop — Kvist ensures continued
/// progress by continuing the run.
pub fn continuation_decision(stop_reason: StopReason) -> ContinuationDecision {
    match stop_reason {
        StopReason::Completed => ContinuationDecision::Finished,
        StopReason::Cancelled
        | StopReason::Stalled
        | StopReason::GuidanceRequired
        | StopReason::Limit(_) => ContinuationDecision::Blocked,
        StopReason::NoWork | StopReason::Failed => ContinuationDecision::Continue,
    }
}

/// Live metrics from the Skott session, derived from its event stream.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkottMetrics {
    /// Model turns begun.
    pub turns: u32,
    /// Tool calls proposed by the model.
    pub tools_executed: u32,
    /// Cumulative input tokens (provider-reported or estimated).
    pub input_tokens: u64,
    /// Cumulative output tokens.
    pub output_tokens: u64,
    /// Tokens the model currently holds in context.
    pub context_tokens: usize,
    /// The model context limit, in tokens.
    pub context_limit: usize,
    /// Successful context reductions (compactions).
    pub compactions: u64,
    /// Live output tokens per observed decode second.
    pub generation_tokens_per_sec: f64,
    /// Average output tokens per decode second.
    pub average_tokens_per_sec: f64,
    /// Cumulative provider tokens observed across the session.
    pub total_tokens: u64,
    /// Wall-clock seconds since the run started.
    pub elapsed_secs: f64,
}

impl SkottMetrics {
    /// A one-line summary for the compact Kvist view.
    pub fn summary_line(&self) -> String {
        format!(
            "turns {} · tools {} · tokens {}/{} (context {}/{}) · {} compactions · {elapsed:.1}s",
            self.turns,
            self.tools_executed,
            self.input_tokens,
            self.output_tokens,
            self.context_tokens,
            self.context_limit,
            self.compactions,
            elapsed = self.elapsed_secs,
        )
    }
}

/// A bounded, human-readable log of Skott events for the detailed view.
#[derive(Debug, Clone, Default)]
pub struct SkottEventLog {
    lines: Vec<String>,
    /// Buffered reasoning text, accumulated until a non-reasoning event or
    /// session end triggers a flush. This matches standalone Skott's behavior
    /// of rendering reasoning as complete paragraphs rather than fragmented
    /// word-by-word output.
    pending_reasoning: String,
}

/// The maximum retained detailed-view lines; older lines are dropped.
const MAX_LOG_LINES: usize = 2000;

impl SkottEventLog {
    /// Appends one event line, keeping the log bounded.
    pub fn push(&mut self, line: String) {
        if line.is_empty() {
            return;
        }
        self.lines.push(line);
        if self.lines.len() > MAX_LOG_LINES {
            let excess = self.lines.len() - MAX_LOG_LINES;
            self.lines.drain(..excess);
        }
    }

    /// The retained event lines, oldest first.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Whether any events have been observed.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Flushes any buffered reasoning text as a single paragraph.
    /// Call this before rendering or when a non-reasoning event arrives.
    pub fn flush_reasoning(&mut self) {
        if !self.pending_reasoning.is_empty() {
            let text = std::mem::take(&mut self.pending_reasoning);
            // Remove leading/trailing whitespace and newlines
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                self.push(trimmed.to_owned());
            }
        }
    }
}

/// The result of a Skott session, reconstructed from its event stream.
#[derive(Debug, Clone, Default)]
pub struct SkottResult {
    /// The final answer text, when the loop produced one.
    pub answer: Option<String>,
    /// The observed stopping cause, when the prompt loop exited.
    pub stop_reason: Option<StopReason>,
    /// A terminal failure diagnostic, if the prompt did not complete.
    pub failure: Option<String>,
    /// Whether the prompt loop has returned control to the caller.
    pub finished: bool,
}

impl SkottResult {
    /// Kvist's completion policy for this result, once the loop has exited.
    pub fn decision(&self) -> Option<ContinuationDecision> {
        self.stop_reason.map(continuation_decision)
    }
}

/// Renders one Skott event as a bounded, human-readable progress line.
fn describe_event(event: &Event) -> String {
    match event {
        Event::Control(notice) => {
            format!("control: guidance #{} {:?}", notice.id.0, notice.state)
        }
        Event::TurnStart { model } => format!("turn started ({model})"),
        Event::AttemptStart { attempt } => format!("attempt {attempt}"),
        Event::Reasoning(text) => format!("reasoning: {}", truncate(text, 200)),
        Event::Text(text) => truncate(text, 400),
        Event::ToolCall { description, name } => format!("tool call: {name} — {description}"),
        Event::ToolResult {
            description,
            name,
            failed,
        } => format!(
            "tool result: {name} — {description} ({})",
            if *failed { "failed" } else { "ok" }
        ),
        Event::ToolFailure {
            description, name, ..
        } => format!("tool failure: {name} — {description}"),
        Event::Finished { message } => format!("finished: {}", truncate(message, 400)),
        Event::PromptEnd {
            exhausted,
            cancelled,
            reason,
        } => format!(
            "prompt ended (reason: {:?}, exhausted: {exhausted}, cancelled: {cancelled})",
            reason
        ),
        Event::Failed(message) => format!("failed: {}", truncate(message, 400)),
        Event::Note(text) => format!("note: {}", truncate(text, 200)),
        Event::Progress {
            input_tokens,
            output_tokens,
            context_tokens,
            context_limit,
            compactions,
            elapsed_secs,
            ..
        } => format!(
            "progress: {input_tokens} in / {output_tokens} out · context {context_tokens}/{context_limit} · {compactions} compactions · {elapsed_secs:.1}s"
        ),
    }
}

/// Bounds a progress fragment to `max` characters, marking truncation.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let truncated: String = text.chars().take(max).collect();
        format!("{truncated}…")
    }
}

/// Converts a Skott error into Kvist's session-launch error.
fn skott_error(error: skott::Error) -> KvistError {
    KvistError::SkottLaunchFailed {
        reason: error.describe(),
    }
}

/// Embedded Skott session within Kvist.
///
/// Owns the Skott session lifecycle (worker thread, event channel, and prompt
/// sender) and the Kvist-side projections of its progress: live metrics, a
/// bounded detailed event log, and the reconstructed result. Kvist pumps the
/// session on its UI thread and renders either the detailed Skott view or the
/// compact Kvist view.
pub struct EmbeddedSkott {
    handle: Option<SessionHandle>,
    receiver: Option<mpsc::Receiver<Event>>,
    prompts: Option<PromptSender>,
    metrics: SkottMetrics,
    log: SkottEventLog,
    result: SkottResult,
    active_pane: ActivePane,
    model_id: Option<String>,
    started: Option<Instant>,
}

impl Default for EmbeddedSkott {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddedSkott {
    /// Creates an idle embedded Skott with no active session.
    pub fn new() -> Self {
        Self {
            handle: None,
            receiver: None,
            prompts: None,
            metrics: SkottMetrics::default(),
            log: SkottEventLog::default(),
            result: SkottResult::default(),
            active_pane: ActivePane::Kvist,
            model_id: None,
            started: None,
        }
    }

    /// Starts a Skott agent session for `model` using `config`.
    ///
    /// Builds the sandbox tool registry, executor, model transport, context
    /// window, and conversation, then launches the Skott worker through
    /// [`skott::start_session`]. The returned instance owns the live session
    /// and opens on the detailed Skott view. `recorder` optionally captures a
    /// durable session record; pass `None` for an in-memory session only.
    pub fn start(
        config: Config,
        model: &Model,
        effort: ReasoningEffort,
        max_turns: Option<u32>,
        limits: RunLimits,
        recorder: Option<Box<dyn Recorder>>,
    ) -> Result<Self> {
        let workdir = config.working_directory.clone();
        let registry = ToolRegistry::resolve_for_workspace(
            config.tool_policy.clone(),
            &config.tool_profiles,
            &HostProbe,
            None,
            &workdir,
        )
        .map_err(skott_error)?;
        let tool_defs = registry.tool_definitions();
        let executor = SandboxExecutor::new(registry, config.sandbox.clone(), workdir);
        let budgets = model
            .resolve_budgets(None, None, limits)
            .map_err(skott_error)?;
        let transport = model.transport().map_err(skott_error)?;
        let retry = model.retry_policy();
        let session = AgentSession::new(
            model.clone(),
            effort,
            tool_defs,
            system_prompt(&config.tool_policy.write_root),
        );
        let context = ContextManager::for_model(budgets.context_limit);
        let (handle, receiver, prompts) = start_session(
            session,
            transport,
            executor,
            context,
            recorder,
            retry,
            max_turns,
            budgets.limits,
        )
        .map_err(skott_error)?;
        Ok(Self {
            handle: Some(handle),
            receiver: Some(receiver),
            prompts: Some(prompts),
            metrics: SkottMetrics::default(),
            log: SkottEventLog::default(),
            result: SkottResult::default(),
            active_pane: ActivePane::Skott,
            model_id: Some(model.id.clone()),
            started: Some(Instant::now()),
        })
    }

    /// Drains pending Skott events, updating metrics, the detailed log, and the
    /// result. Returns `true` when at least one event was observed.
    pub fn pump(&mut self) -> bool {
        let Some(receiver) = &self.receiver else {
            return false;
        };
        // Collect before observing: `observe` mutably borrows `self`, so the
        // immutable borrow of `self.receiver` must end first.
        let events: Vec<Event> = receiver.try_iter().collect();
        let mut changed = false;
        for event in events {
            self.observe(event);
            changed = true;
        }
        // Flush any buffered reasoning so the UI can render it
        if changed {
            self.log.flush_reasoning();
        }
        changed
    }

    /// Applies one event to the metrics, detailed log, and result.
    fn observe(&mut self, event: Event) {
        match &event {
            Event::TurnStart { .. } => {
                // Flush any buffered reasoning before starting a new turn
                self.log.flush_reasoning();
                self.metrics.turns = self.metrics.turns.saturating_add(1);
            }
            Event::ToolCall { .. } => {
                self.log.flush_reasoning();
                self.metrics.tools_executed = self.metrics.tools_executed.saturating_add(1);
            }
            Event::ToolResult { .. } => {
                self.log.flush_reasoning();
            }
            Event::ToolFailure { .. } => {
                self.log.flush_reasoning();
            }
            Event::Progress {
                input_tokens,
                output_tokens,
                context_tokens,
                context_limit,
                compactions,
                generation_tokens_per_sec,
                average_tokens_per_sec,
                total_tokens,
                elapsed_secs,
                ..
            } => {
                self.metrics.input_tokens = *input_tokens;
                self.metrics.output_tokens = *output_tokens;
                self.metrics.context_tokens = *context_tokens;
                self.metrics.context_limit = *context_limit;
                self.metrics.compactions = *compactions;
                self.metrics.generation_tokens_per_sec = *generation_tokens_per_sec;
                self.metrics.average_tokens_per_sec = *average_tokens_per_sec;
                self.metrics.total_tokens = *total_tokens;
                self.metrics.elapsed_secs = *elapsed_secs;
                // Progress is shown in the header metrics, not the event log.
                return;
            }
            Event::Text(text) => {
                // Flush reasoning before answer text, matching Skott's order
                self.log.flush_reasoning();
                self.log.push(text.clone());
            }
            // `Finished` and `PromptEnd` are mutually exclusive: the runtime
            // emits `Finished` when a run ends with an answer and `PromptEnd`
            // on every other loop exit. Both are terminal, so each sets the
            // stop reason; `Finished` is an unambiguous completion.
            Event::Finished { message } => {
                self.log.flush_reasoning();
                self.result.answer = Some(message.clone());
                self.result.stop_reason = Some(StopReason::Completed);
                self.result.finished = true;
            }
            Event::PromptEnd { reason, .. } => {
                self.log.flush_reasoning();
                self.result.stop_reason = Some(*reason);
                self.result.finished = true;
            }
            Event::Failed(message) => {
                self.log.flush_reasoning();
                self.result.failure = Some(message.clone());
                self.result.stop_reason = Some(StopReason::Failed);
                self.result.finished = true;
            }
            Event::Reasoning(text) => {
                // Buffer reasoning text; it will be flushed as a complete
                // paragraph when the next non-reasoning event arrives.
                self.log.pending_reasoning.push_str(text);
                return;
            }
            _ => {
                self.log.flush_reasoning();
            }
        }
        self.log.push(describe_event(&event));
    }

    /// Submits a prompt to the running session.
    ///
    /// A new prompt starts a fresh result: the previous terminal state (answer,
    /// stop reason, failure) no longer applies, so the continuation policy must
    /// not re-fire on it. Session metrics accumulate across prompts.
    pub fn submit(&mut self, prompt: String) -> Result<()> {
        let prompts =
            self.prompts
                .as_ref()
                .ok_or_else(|| KvistError::SkottCommunicationFailed {
                    reason: "no active Skott session".into(),
                })?;
        self.result = SkottResult::default();
        prompts
            .send(prompt)
            .map(|_| ())
            .map_err(|error| KvistError::SkottCommunicationFailed {
                reason: error.describe(),
            })
    }

    /// Requests cancellation of the active turn; observed effects are retained.
    pub fn cancel(&mut self) {
        if let Some(handle) = &self.handle {
            handle.cancel();
        }
    }

    /// Shuts the session down permanently, including queued prompts.
    pub fn shutdown(&mut self) {
        if let Some(handle) = &self.handle {
            handle.shutdown();
        }
    }

    /// Handles keyboard events for pane switching.
    ///
    /// Returns `true` when the event was consumed (a pane switch). Ctrl+Tab
    /// toggles between the two views; Ctrl+PageUp / Ctrl+PageDown and
    /// Alt+Left / Alt+Right select a specific view. Plain PageUp/PageDown,
    /// Left/Right and Home/End are never consumed here, so the Kvist
    /// transcript keeps its normal scrolling.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Tab {
            self.active_pane = match self.active_pane {
                ActivePane::Kvist => ActivePane::Skott,
                ActivePane::Skott => ActivePane::Both,
                ActivePane::Both => ActivePane::Kvist,
            };
            return true;
        }
        if control && key.code == KeyCode::PageUp {
            self.active_pane = ActivePane::Kvist;
            return true;
        }
        if control && key.code == KeyCode::PageDown {
            self.active_pane = ActivePane::Skott;
            return true;
        }
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if alt && key.code == KeyCode::Left {
            self.active_pane = ActivePane::Kvist;
            return true;
        }
        if alt && key.code == KeyCode::Right {
            self.active_pane = ActivePane::Skott;
            return true;
        }
        false
    }

    /// Sets the active pane directly.
    pub fn set_active_pane(&mut self, pane: ActivePane) {
        self.active_pane = pane;
    }

    /// The active view.
    pub fn active_pane(&self) -> ActivePane {
        self.active_pane
    }

    /// Live session metrics.
    pub fn metrics(&self) -> &SkottMetrics {
        &self.metrics
    }

    /// The detailed event log.
    pub fn event_log(&self) -> &SkottEventLog {
        &self.log
    }

    /// The reconstructed session result.
    pub fn result(&self) -> &SkottResult {
        &self.result
    }

    /// Whether a session is active.
    pub fn is_active(&self) -> bool {
        self.receiver.is_some()
    }

    /// Whether the Skott worker is still alive (including idle prompt waits).
    pub fn is_running(&self) -> bool {
        self.handle
            .as_ref()
            .is_some_and(|handle| handle.is_running())
    }

    /// The selected model id, when a session is active.
    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    /// Wall-clock duration since the session started, when active.
    pub fn elapsed(&self) -> Option<std::time::Duration> {
        self.started.map(|started| started.elapsed())
    }
}

impl Drop for EmbeddedSkott {
    fn drop(&mut self) {
        // A dropped Kvist session must not leave the Skott worker executing.
        if let Some(handle) = &self.handle {
            handle.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::Duration;

    use sav::{
        CancellationToken, FinishReason, LocalModelProvider, ModelRequest, ModelStreamEvent,
        ModelTransport, ModelTurn, ModelUsage, ToolIntent,
    };
    use skott::ToolOutcome;
    use skott::session::ToolExecutor;

    /// A deterministic transport that answers every turn with a fixed text and
    /// no tool intents, so a session completes in a single turn.
    #[derive(Default)]
    struct StubTransport {
        calls: Mutex<u32>,
    }

    impl ModelTransport for StubTransport {
        fn complete(
            &self,
            _request: &ModelRequest,
            _cancellation: &CancellationToken,
        ) -> sav::Result<ModelTurn> {
            *self.calls.lock().unwrap() += 1;
            Ok(turn())
        }
        fn stream(
            &self,
            _request: &ModelRequest,
            _cancellation: &CancellationToken,
            on_event: &mut dyn FnMut(ModelStreamEvent) -> sav::Result<()>,
        ) -> sav::Result<ModelTurn> {
            *self.calls.lock().unwrap() += 1;
            on_event(ModelStreamEvent::TextDelta("done".into()))?;
            Ok(turn())
        }
        fn deadline(&self) -> Duration {
            Duration::from_secs(30)
        }
    }

    fn turn() -> ModelTurn {
        ModelTurn {
            text: "done".into(),
            reasoning: None,
            tool_intents: vec![],
            finish_reason: FinishReason::Stop,
            provider: LocalModelProvider::LlamaServer,
            model: "fixture".into(),
            response_id: None,
            provider_request_id: None,
            usage: Some(ModelUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
            }),
        }
    }

    /// A no-op executor; the stub transport proposes no tools, so it is never
    /// dispatched.
    struct StubExecutor;

    impl ToolExecutor for StubExecutor {
        fn execute(
            &self,
            _intent: &ToolIntent,
            _cancellation: &CancellationToken,
        ) -> skott::Result<ToolOutcome> {
            Ok(ToolOutcome::rejected_with(
                "stub executor: no tools expected",
            ))
        }
    }

    fn test_config() -> Config {
        let model = Model {
            id: "fixture".into(),
            provider: skott::ModelProvider::LlamaServer,
            base_url: "http://127.0.0.1:1".into(),
            model: "fixture".into(),
            is_default: true,
            context_limit: Some(4096),
            response_reserve: Some(512),
            deadline_secs: 30,
            max_attempts: 1,
            retry_base_delay_secs: 0,
            retry_max_delay_secs: 0,
            cadence_timeout_secs: 0,
        };
        Config::from_parts(
            PathBuf::from("."),
            skott::ModelProvider::LlamaServer,
            vec![model],
            skott::ToolPolicy::default(),
        )
        .expect("test config")
    }

    fn test_model(config: &Config) -> &Model {
        &config.models[0]
    }

    /// Starts a session with a stub transport by driving the same construction
    /// path as [`EmbeddedSkott::start`], but injecting the stub transport and
    /// executor in place of the real model transport and sandbox executor.
    fn start_stubbed() -> (EmbeddedSkott, PromptSender) {
        let config = test_config();
        let model = test_model(&config).clone();
        let budgets = model
            .resolve_budgets(None, None, RunLimits::default())
            .unwrap();
        let session = AgentSession::new(
            model.clone(),
            ReasoningEffort::None,
            vec![],
            system_prompt(&config.tool_policy.write_root),
        );
        let context = ContextManager::for_model(budgets.context_limit);
        let (handle, receiver, prompts) = start_session(
            session,
            StubTransport::default(),
            StubExecutor,
            context,
            None,
            model.retry_policy(),
            None,
            budgets.limits,
        )
        .unwrap();
        let mut embedded = EmbeddedSkott::new();
        embedded.handle = Some(handle);
        embedded.receiver = Some(receiver);
        embedded.prompts = Some(prompts.clone());
        embedded.model_id = Some(model.id);
        embedded.started = Some(Instant::now());
        embedded.active_pane = ActivePane::Skott;
        (embedded, prompts)
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    #[test]
    fn new_session_is_idle_on_the_kvist_pane() {
        let mut skott = EmbeddedSkott::new();
        assert!(!skott.is_active());
        assert!(!skott.is_running());
        assert_eq!(skott.active_pane(), ActivePane::Kvist);
        assert!(!skott.pump());
        assert!(skott.event_log().is_empty());
    }

    #[test]
    fn pane_switching_uses_ctrl_pagedown_and_pagup() {
        let mut skott = EmbeddedSkott::new();
        assert!(skott.handle_key(key(KeyCode::PageDown, KeyModifiers::CONTROL)));
        assert_eq!(skott.active_pane(), ActivePane::Skott);
        assert!(skott.handle_key(key(KeyCode::PageUp, KeyModifiers::CONTROL)));
        assert_eq!(skott.active_pane(), ActivePane::Kvist);
    }

    #[test]
    fn pane_switching_uses_alt_arrows() {
        let mut skott = EmbeddedSkott::new();
        assert!(skott.handle_key(key(KeyCode::Right, KeyModifiers::ALT)));
        assert_eq!(skott.active_pane(), ActivePane::Skott);
        assert!(skott.handle_key(key(KeyCode::Left, KeyModifiers::ALT)));
        assert_eq!(skott.active_pane(), ActivePane::Kvist);
    }

    #[test]
    fn unrelated_keys_are_not_consumed() {
        let mut skott = EmbeddedSkott::new();
        assert!(!skott.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE)));
        assert!(!skott.handle_key(key(KeyCode::PageUp, KeyModifiers::NONE)));
        assert!(!skott.handle_key(key(KeyCode::Left, KeyModifiers::NONE)));
    }

    #[test]
    fn continuation_decision_partitions_stop_reasons() {
        assert_eq!(
            continuation_decision(StopReason::Completed),
            ContinuationDecision::Finished
        );
        assert_eq!(
            continuation_decision(StopReason::Cancelled),
            ContinuationDecision::Blocked
        );
        assert_eq!(
            continuation_decision(StopReason::Stalled),
            ContinuationDecision::Blocked
        );
        assert_eq!(
            continuation_decision(StopReason::GuidanceRequired),
            ContinuationDecision::Blocked
        );
        for resource in [
            skott::BudgetResource::WallTime,
            skott::BudgetResource::ModelTokens,
            skott::BudgetResource::Turns,
            skott::BudgetResource::ModelAttempts,
            skott::BudgetResource::Context,
        ] {
            assert_eq!(
                continuation_decision(StopReason::Limit(resource)),
                ContinuationDecision::Blocked
            );
        }
        assert_eq!(
            continuation_decision(StopReason::NoWork),
            ContinuationDecision::Continue
        );
        assert_eq!(
            continuation_decision(StopReason::Failed),
            ContinuationDecision::Continue
        );
    }

    #[test]
    fn a_stubbed_session_streams_a_completed_turn_and_metrics() {
        let (mut skott, prompts) = start_stubbed();
        prompts.send("do the task".into()).unwrap();
        // Pump until the prompt loop returns control (bounded to avoid a hang).
        let deadline = Instant::now() + Duration::from_secs(5);
        while !skott.result().finished && Instant::now() < deadline {
            skott.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(skott.result().finished, "session should finish");
        assert_eq!(
            skott.result().stop_reason,
            Some(StopReason::Completed),
            "a single answered turn completes"
        );
        assert_eq!(
            skott.result().decision(),
            Some(ContinuationDecision::Finished)
        );
        assert!(skott.result().answer.is_some());
        assert!(skott.metrics().turns >= 1);
        assert!(skott.metrics().input_tokens >= 10);
        assert!(!skott.event_log().is_empty());
        assert_eq!(skott.active_pane(), ActivePane::Skott);
    }

    #[test]
    fn submit_without_a_session_fails() {
        let mut skott = EmbeddedSkott::new();
        assert!(skott.submit("prompt".into()).is_err());
    }

    #[test]
    fn submit_resets_the_result_so_continuation_does_not_refire() {
        let (mut skott, prompts) = start_stubbed();
        prompts.send("first".into()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !skott.result().finished && Instant::now() < deadline {
            skott.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(skott.result().finished, "the first prompt should finish");
        // A new prompt resets the result; the prior terminal state must not
        // re-trigger the continuation policy on the next pump.
        skott.submit("second".into()).unwrap();
        assert!(!skott.result().finished);
        assert_eq!(skott.result().stop_reason, None);
        assert_eq!(skott.result().decision(), None);
    }

    #[test]
    fn describe_event_renders_every_variant_bounded() {
        let long = "x".repeat(10_000);
        for event in [
            Event::TurnStart { model: "m".into() },
            Event::AttemptStart { attempt: 3 },
            Event::Reasoning(long.clone()),
            Event::Text(long.clone()),
            Event::ToolCall {
                description: "d".into(),
                name: "read_file".into(),
            },
            Event::ToolResult {
                description: "d".into(),
                name: "read_file".into(),
                failed: true,
            },
            Event::Finished {
                message: long.clone(),
            },
            Event::PromptEnd {
                exhausted: false,
                cancelled: false,
                reason: StopReason::Completed,
            },
            Event::Failed(long.clone()),
            Event::Note(long.clone()),
            Event::Progress {
                token_accounting: Default::default(),
                input_tokens: 1,
                output_tokens: 2,
                context_tokens: 3,
                context_limit: 4,
                context_utilization: 0.5,
                compaction_progress: 0.1,
                compactions: 0,
                generation_tokens_per_sec: 1.0,
                average_tokens_per_sec: 1.0,
                total_tokens: 3,
                elapsed_secs: 1.5,
            },
        ] {
            let line = describe_event(&event);
            assert!(!line.is_empty());
            assert!(
                line.chars().count() <= 4000,
                "event line must stay bounded: {line}"
            );
        }
    }

    #[test]
    fn reasoning_is_buffered_and_flushed_as_paragraph() {
        let mut log = SkottEventLog::default();
        // Simulate fragmented reasoning events
        log.pending_reasoning.push_str("This is ");
        log.pending_reasoning.push_str("a test of ");
        log.pending_reasoning.push_str("reasoning buffering.");
        // Flush when a non-reasoning event arrives
        log.flush_reasoning();
        // Should be one line, not three fragmented lines
        assert_eq!(log.lines().len(), 1);
        assert_eq!(
            log.lines()[0],
            "This is a test of reasoning buffering."
        );
    }

    #[test]
    fn reasoning_buffer_survives_multiple_fragments() {
        let mut log = SkottEventLog::default();
        log.pending_reasoning.push_str("First part.");
        log.pending_reasoning.push_str(" Second part.");
        log.pending_reasoning.push_str(" Third part.");
        log.flush_reasoning();
        assert_eq!(log.lines().len(), 1);
        assert_eq!(
            log.lines()[0],
            "First part. Second part. Third part."
        );
    }

    #[test]
    fn event_log_is_bounded() {
        let mut log = SkottEventLog::default();
        for i in 0..(MAX_LOG_LINES + 500) {
            log.push(format!("line {i}"));
        }
        assert_eq!(log.lines().len(), MAX_LOG_LINES);
        assert_eq!(log.lines().first(), Some(&"line 500".to_string()));
    }

    #[test]
    fn metrics_summary_line_reports_counts() {
        let metrics = SkottMetrics {
            turns: 2,
            tools_executed: 3,
            input_tokens: 100,
            output_tokens: 50,
            context_tokens: 150,
            context_limit: 4096,
            compactions: 1,
            generation_tokens_per_sec: 12.5,
            average_tokens_per_sec: 10.0,
            total_tokens: 150,
            elapsed_secs: 4.25,
        };
        let line = metrics.summary_line();
        assert!(line.contains("turns 2"));
        assert!(line.contains("tools 3"));
        assert!(line.contains("100/50"));
        assert!(line.contains("150/4096"));
        assert!(line.contains("1 compactions"));
    }
}
