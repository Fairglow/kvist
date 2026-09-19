//! Rolling context management for the agent loop.
//!
//! The model conversation grows by one turn every step. `ContextManager`
//! estimates the token size of the conversation that will next be sent to the
//! model, reports when it crosses a warm-up threshold, and compacts the oldest
//! completed turns into a concise rolling summary while the most recent turns
//! stay in full.
//!
//! Compaction only ever removes material from the *model* context. The full
//! transcript, including every reasoning trace, is preserved separately in the
//! durable session log (`crate::session_log`); a compacted turn is never lost,
//! it is simply moved from the live context into the record. This keeps the
//! model's context bounded without weakening the audit trail Kvist relies on.

use agent_runtime::ModelMessage;

/// Approximate tokens per character (the widely used ~4 chars/token rule).
const CHARS_PER_TOKEN: usize = 4;
/// Per-message framing overhead in the request, in tokens.
const MESSAGE_OVERHEAD_TOKENS: usize = 4;
/// Per tool-definition framing overhead in the request, in tokens.
const TOOL_DEFINITION_OVERHEAD_TOKENS: usize = 8;
/// Maximum length of the rolling summary before it is trimmed.
const MAX_SUMMARY_CHARS: usize = 4_000;

/// Estimates the token size of a block of text using the ~4-chars-per-token
/// heuristic. Cheap and deterministic; the provider-reported usage is
/// authoritative for speed stats.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(CHARS_PER_TOKEN)
}

fn estimate_intent_tokens(intent: &agent_runtime::ToolIntent) -> usize {
    match serde_json::to_string(intent) {
        Ok(json) => estimate_tokens(&json) + MESSAGE_OVERHEAD_TOKENS,
        Err(_) => MESSAGE_OVERHEAD_TOKENS,
    }
}

fn message_tokens(message: &ModelMessage) -> usize {
    let mut total = MESSAGE_OVERHEAD_TOKENS;
    match message {
        ModelMessage::System(text) | ModelMessage::User(text) => total += estimate_tokens(text),
        ModelMessage::Assistant { text, tool_intents } => {
            total += estimate_tokens(text);
            for intent in tool_intents {
                total += estimate_intent_tokens(intent);
            }
        }
        ModelMessage::ToolResult { content, .. } => total += estimate_tokens(content),
    }
    total
}

/// Estimates the token size of the message list that would be sent as one
/// request, accounting for each message plus the always-present tool
/// definitions. This is the figure `ContextManager` tracks against its limits.
pub fn estimate_messages(messages: &[ModelMessage], tool_definitions: usize) -> usize {
    let mut total = tool_definitions * TOOL_DEFINITION_OVERHEAD_TOKENS;
    for message in messages {
        total += message_tokens(message);
    }
    total
}

fn utilization(used: usize, limit: usize) -> f64 {
    if limit == 0 {
        0.0
    } else {
        used as f64 / limit as f64
    }
}

/// Information about a compaction, returned to the caller (and the UI) so it
/// can be logged and shown as progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compaction {
    /// How many completed turns were rolled into the summary.
    pub compacted_turns: usize,
    /// Length of the rolling summary after compaction, in characters.
    pub summary_chars: usize,
}

/// The default context window assumed when the model is unknown.
pub const DEFAULT_CONTEXT_TOKENS: usize = 8_192;

/// Rolling context manager: estimates context size and compacts the oldest
/// completed turns into a summary.
#[derive(Debug, Clone)]
pub struct ContextManager {
    /// Hard limit on the model context, in tokens.
    limit_tokens: usize,
    /// Start compacting above this many tokens (75% of the limit by default).
    warmup_tokens: usize,
    /// The most recent completed turns always stay in full, never summarized.
    keep_full_turns: usize,
    /// Rolling summary text representing compacted history.
    summary: String,
}

impl ContextManager {
    /// Builds a manager for a model context window of `limit_tokens`.
    ///
    /// Compaction begins at 75% of the window and always keeps the last
    /// `keep_full_turns` completed turns in full.
    pub fn new(limit_tokens: usize, keep_full_turns: usize) -> Self {
        let warmup_tokens = limit_tokens.div_ceil(4) * 3;
        Self::with_bounds(limit_tokens, warmup_tokens, keep_full_turns)
    }

    /// Builds a manager with explicit warm-up and hard limits. Exposed for
    /// tests and for tuning on models with small windows.
    pub fn with_bounds(limit_tokens: usize, warmup_tokens: usize, keep_full_turns: usize) -> Self {
        debug_assert!(limit_tokens > warmup_tokens, "limit must exceed warm-up");
        debug_assert!(
            keep_full_turns >= 1,
            "at least one turn must be kept in full"
        );
        ContextManager {
            limit_tokens,
            warmup_tokens,
            keep_full_turns,
            summary: String::new(),
        }
    }

    /// The hard context limit, in tokens.
    pub fn limit_tokens(&self) -> usize {
        self.limit_tokens
    }

    /// The warm-up threshold at or above which compaction begins, in tokens.
    pub fn warmup(&self) -> usize {
        self.warmup_tokens
    }

    /// How many completed turns are always kept in full.
    pub fn keep_full_turns(&self) -> usize {
        self.keep_full_turns
    }

    /// The current rolling summary text.
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Whether the estimated `context_tokens` has crossed the warm-up threshold
    /// and compaction should be performed before the next request.
    pub fn should_compact(&self, context_tokens: usize) -> bool {
        context_tokens > self.warmup_tokens
    }

    /// How far compaction is from the hard limit, as a fraction in
    /// `0.0..=1.0`. Zero at or below warm-up; one at or above the limit. This is
    /// the value a compaction progress bar renders.
    pub fn compaction_progress(&self, context_tokens: usize) -> f64 {
        if context_tokens <= self.warmup_tokens {
            0.0
        } else if context_tokens >= self.limit_tokens {
            1.0
        } else {
            (context_tokens - self.warmup_tokens) as f64
                / (self.limit_tokens - self.warmup_tokens) as f64
        }
    }

    /// Whether the estimated `context_tokens` has reached or passed the limit,
    /// meaning a request would be rejected by the model.
    pub fn at_limit(&self, context_tokens: usize) -> bool {
        context_tokens >= self.limit_tokens
    }

    /// The current context utilization fraction against the hard limit
    /// (`0.0..=1.0+`), for the context bargraph.
    pub fn utilization(&self, context_tokens: usize) -> f64 {
        utilization(context_tokens, self.limit_tokens)
    }

    /// Compacts `messages` (excluding the leading system message, which is
    /// preserved) by rolling the oldest completed turns into the summary and
    /// keeping the most recent turns in full. Returns the condensed message
    /// list and compaction metadata. No-ops when there is nothing past
    /// `keep_full_turns` to roll.
    ///
    /// Compaction keeps as many of the most recent turns in full as fit under
    /// the hard `limit_tokens`, rolling the rest into the summary. It starts by
    /// keeping `keep_full_turns` recent turns in full, then keeps rolling more
    /// into the summary (reducing how many recent turns stay full) until the
    /// estimated context is bounded. This guarantees a long-running session's
    /// live context stays under the window even when individual turns are large;
    /// the single most recent turn is always kept in full as a best effort,
    /// since that is the only case where keeping it cannot bring the context
    /// under the limit.
    pub fn compact(
        &mut self,
        messages: &[ModelMessage],
        tool_definitions: usize,
    ) -> (Vec<ModelMessage>, Compaction) {
        let mut turns: Vec<&ModelMessage> = Vec::new();
        let mut system: Option<&ModelMessage> = None;
        for message in messages {
            match message {
                ModelMessage::System(_) => system = Some(message),
                _ => turns.push(message),
            }
        }

        let segments = segment_turns(&turns);
        if segments.len() <= self.keep_full_turns {
            let condensed = keep_segments(messages, &segments, self.keep_full_turns);
            return (
                condensed,
                Compaction {
                    compacted_turns: 0,
                    summary_chars: self.summary.chars().count(),
                },
            );
        }

        // The single most recent turn must always stay in full so the model
        // keeps its immediate context and any pending tool continuity; everything
        // older rolls into the summary. Keep as many recent turns in full as fit
        // under the hard limit, compacting more as needed.
        const MIN_KEEP_FULL_TURNS: usize = 1;
        let total = segments.len();
        let mut keep = self
            .keep_full_turns
            .min(total.saturating_sub(MIN_KEEP_FULL_TURNS));
        keep = keep.max(MIN_KEEP_FULL_TURNS);

        // Accumulated summary lines, oldest first; appended to as `keep` shrinks,
        // so each pass folds one more older segment into the rolling summary.
        // Each segment is summarized exactly once: `summarized` tracks how many
        // leading segments already have a line, so a second pass only adds the
        // newly compacted segment instead of re-duplicating the earlier ones.
        let mut summary_parts: Vec<String> = Vec::new();
        let mut summarized = 0usize;
        loop {
            let (compactable, kept) = segments.split_at(total - keep);
            for segment in &compactable[summarized..] {
                let line = turn_summary(segment);
                if !line.is_empty() {
                    summary_parts.push(line);
                }
            }
            summarized = compactable.len();
            let rolled = bounded_summary(&summary_parts);

            let mut condensed = Vec::new();
            if let Some(sys) = system {
                condensed.push(sys.clone());
            }
            if !rolled.is_empty() {
                condensed.push(ModelMessage::User(format!(
                    "## Summary of prior work ({} earlier turn(s), kept in the session log):\n{}",
                    compactable.len(),
                    rolled
                )));
            }
            for segment in kept {
                for &message in segment {
                    condensed.push(message.clone());
                }
            }

            let summary_chars = rolled.chars().count();
            if estimate_messages(&condensed, tool_definitions) < self.limit_tokens
                || keep <= MIN_KEEP_FULL_TURNS
            {
                self.summary = rolled;
                return (
                    condensed,
                    Compaction {
                        compacted_turns: total - keep,
                        summary_chars,
                    },
                );
            }
            keep -= 1;
        }
    }
}

/// Joins accumulated summary lines (oldest first) and trims the result to
/// `MAX_SUMMARY_CHARS`, keeping the most recent content so the rolling summary
/// never grows past its bound.
fn bounded_summary(parts: &[String]) -> String {
    let mut joined = parts.join("\n");
    if joined.chars().count() > MAX_SUMMARY_CHARS {
        let trim = joined.chars().count() - MAX_SUMMARY_CHARS;
        joined = joined.chars().skip(trim).collect();
        joined.push('\u{2026}');
    }
    joined
}

/// Splits an ordered conversation into segments, each starting at a `User`
/// message and running through the assistant turn(s) and tool results that
/// follow it.
fn segment_turns<'a>(messages: &'a [&'a ModelMessage]) -> Vec<Vec<&'a ModelMessage>> {
    let mut segments: Vec<Vec<&'a ModelMessage>> = Vec::new();
    let mut current: Vec<&'a ModelMessage> = Vec::new();
    for &message in messages {
        if matches!(message, ModelMessage::User(_)) && !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        }
        current.push(message);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

/// Rebuilds the message list keeping only the last `keep` segments in full and
/// dropping the rest (the system message is preserved at the front).
fn keep_segments(
    messages: &[ModelMessage],
    segments: &[Vec<&ModelMessage>],
    keep: usize,
) -> Vec<ModelMessage> {
    let drop = segments.len().saturating_sub(keep);
    let mut kept: Vec<ModelMessage> = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        if index >= drop {
            kept.extend(segment.iter().map(|&message| message.clone()));
        }
    }
    match messages
        .iter()
        .find(|m| matches!(m, ModelMessage::System(_)))
    {
        Some(sys) => {
            let mut with_sys = Vec::with_capacity(kept.len() + 1);
            with_sys.push(sys.clone());
            with_sys.extend(kept);
            with_sys
        }
        None => kept,
    }
}

/// Builds a single-line summary for one completed turn segment.
fn turn_summary(segment: &[&ModelMessage]) -> String {
    let mut line = String::new();
    for message in segment {
        match message {
            ModelMessage::User(text) => {
                line.push_str("asked:");
                line.push_str(&truncate(text, 160));
            }
            ModelMessage::Assistant { text, tool_intents } => {
                if line.trim().is_empty() && text.trim().is_empty() {
                    // Nothing yet.
                } else if line.trim().is_empty() {
                    line.push_str(&truncate(text, 120));
                } else {
                    line.push_str(&truncate(text, 80));
                }
                for intent in tool_intents {
                    line.push_str(&format!(" ran {}(=)", intent.name));
                    if let Ok(json) = serde_json::to_string(&intent.arguments) {
                        line.push_str(&truncate(&json, 100));
                    }
                }
            }
            ModelMessage::ToolResult { content, .. } => {
                let preview = truncate(content, 120);
                if preview.is_empty() {
                    line.push_str(" ->");
                } else {
                    line.push_str(&format!(" -> {}", preview));
                }
            }
            _ => {}
        }
    }
    if line.trim().is_empty() {
        String::new()
    } else {
        format!("- {}", line.trim())
    }
}

/// Truncates `text` to at most `max` characters, appending an ellipsis when
/// trimmed so a summary line never grows unbounded.
fn truncate(text: &str, max: usize) -> String {
    let chars: String = text.chars().take(max).collect();
    if chars.chars().count() == text.chars().count() {
        chars
    } else if max == 0 {
        String::new()
    } else {
        format!("...{}", chars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::ToolIntent;
    use serde_json::json;

    fn assistant(text: &str) -> ModelMessage {
        ModelMessage::Assistant {
            text: text.to_owned(),
            tool_intents: vec![ToolIntent {
                id: "c1".to_owned(),
                provider_id: None,
                name: "shell".to_owned(),
                arguments: json!({"command": "echo hi"}),
            }],
        }
    }

    fn result(name: &str, content: &str) -> ModelMessage {
        ModelMessage::ToolResult {
            call_id: "c1".to_owned(),
            name: name.to_owned(),
            content: content.to_owned(),
        }
    }

    fn user(text: &str) -> ModelMessage {
        ModelMessage::User(text.to_owned())
    }

    #[test]
    fn tokens_follow_the_four_chars_per_token_rule() {
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_accounts_for_messages_and_tool_defs() {
        let msgs = vec![user("hello"), assistant("sure")];
        let total = estimate_messages(&msgs, 4);
        assert!(total > 0);
        assert!(estimate_messages(&msgs, 8) > total);
    }

    #[test]
    fn warmup_is_seventy_five_percent_of_limit() {
        let manager = ContextManager::new(1000, 3);
        assert_eq!(manager.warmup(), 750);
        assert!(!manager.should_compact(700));
        assert!(manager.should_compact(800));
    }

    #[test]
    fn progress_is_zero_below_warmup_and_one_at_limit() {
        let manager = ContextManager::with_bounds(1000, 750, 2);
        assert_eq!(manager.compaction_progress(500), 0.0);
        assert_eq!(manager.compaction_progress(750), 0.0);
        assert_eq!(manager.compaction_progress(1_000), 1.0);
        assert!((manager.compaction_progress(875) - 0.5).abs() < 0.01);
    }

    #[test]
    fn nothing_compacts_when_turns_are_few() {
        let mut manager = ContextManager::with_bounds(1000, 200, 3);
        let msgs = vec![
            ModelMessage::System("sys".to_owned()),
            user("a"),
            assistant("b"),
            result("shell", "ok"),
        ];
        let (condensed, compaction) = manager.compact(&msgs, 4);
        assert_eq!(compaction.compacted_turns, 0);
        assert_eq!(condensed.len(), msgs.len());
    }

    #[test]
    fn oldest_turns_roll_into_summary_young_keep_full() {
        let mut manager = ContextManager::with_bounds(10_000, 100, 1);
        let msgs = vec![
            ModelMessage::System("sys".to_owned()),
            user("first task"),
            assistant("doing first"),
            result("shell", "first output"),
            user("second task"),
            assistant("doing second"),
            result("shell", "second output"),
        ];
        let (condensed, compaction) = manager.compact(&msgs, 4);
        assert_eq!(compaction.compacted_turns, 1);
        assert!(manager.summary().contains("first task"));
        assert!(manager.summary().contains("first output"));
        assert!(matches!(condensed[0], ModelMessage::System(_)));
        let joined = condensed
            .iter()
            .map(|m| format!("{m:?}"))
            .collect::<String>();
        assert!(joined.contains("second task"));
        assert!(joined.contains("second output"));
        assert!(condensed.iter().any(|m| matches!(
            m,
            ModelMessage::User(text) if text.contains("Summary of prior work")
        )));
    }

    #[test]
    fn summary_is_bounded() {
        let mut manager = ContextManager::with_bounds(10_000, 100, 1);
        let mut msgs = vec![ModelMessage::System("sys".to_owned())];
        for i in 0..50 {
            let task = format!("task {i}");
            let work = format!("work {i}");
            let result_text = format!("result {i}");
            msgs.push(user(&task));
            msgs.push(assistant(&work));
            msgs.push(result("shell", &result_text));
        }
        let (condensed, _) = manager.compact(&msgs, 4);
        assert!(manager.summary.chars().count() <= MAX_SUMMARY_CHARS + 1);
        assert!(condensed.len() < msgs.len());
    }

    #[test]
    fn compaction_keeps_recent_turns_and_fits_under_the_window() {
        let mut manager = ContextManager::with_bounds(10_000, 100, 3);
        let mut msgs = vec![ModelMessage::System("sys".to_owned())];
        for i in 0..6 {
            msgs.push(user(&format!("task {i}")));
            msgs.push(assistant(&format!("work {i}")));
            msgs.push(result("shell", &format!("result {i}")));
        }
        let (condensed, compaction) = manager.compact(&msgs, 4);
        // More than the kept turns were rolled into the summary, but never more
        // than the configured number of recent turns were forced out of full
        // context: it keeps as many recent turns in full as fit.
        assert!(compaction.compacted_turns > 0);
        assert!(compaction.compacted_turns <= manager.keep_full_turns());
        let joined = condensed
            .iter()
            .map(|m| format!("{m:?}"))
            .collect::<String>();
        assert!(joined.contains("task 5"));
        assert!(manager.summary().contains("task 0"));
        assert!(estimate_messages(&condensed, 4) < manager.limit_tokens());
    }

    #[test]
    fn compaction_summary_contains_each_rolled_turn_exactly_once() {
        // Regression: when the window forces more than one pass of `keep` shrinking,
        // each pass must add only the newly compacted segment; older turn lines must
        // not be duplicated in the rolling summary.
        let mut manager = ContextManager::with_bounds(120, 90, 3);
        let mut msgs = vec![ModelMessage::System("system".to_owned())];
        for i in 0..6 {
            let text = "w".repeat(120);
            msgs.push(user(&format!("ask {i}")));
            msgs.push(assistant(&text));
            msgs.push(result("shell", "o".repeat(40).as_str()));
        }
        let (_, compaction) = manager.compact(&msgs, 4);
        assert!(compaction.compacted_turns >= 2, "force at least two passes");
        let summary = manager.summary().to_owned();
        for i in 0..compaction.compacted_turns {
            let needle = format!("ask {i}");
            let count = summary.matches(&needle).count();
            assert_eq!(
                count, 1,
                "summary line for turn {i} appears {count} times\nsummary:\n{summary}"
            );
        }
    }

    #[test]
    fn compaction_reduces_full_turns_when_the_window_is_tight() {
        // When keeping the configured recent turns would overflow the window,
        // compaction rolls more into the summary, always keeping the single most
        // recent turn in full as the floor.
        let mut manager = ContextManager::with_bounds(120, 90, 3);
        let mut msgs = vec![ModelMessage::System("system".to_owned())];
        for i in 0..6 {
            let text = "w".repeat(120);
            msgs.push(user(&format!("ask {i}")));
            msgs.push(assistant(&text));
            msgs.push(result("shell", "o".repeat(40).as_str()));
        }
        let (_, compaction) = manager.compact(&msgs, 4);
        // keep in [1, keep_full_turns], so compacted turns in
        // [total - keep_full_turns, total - 1].
        assert!(compaction.compacted_turns >= 6 - manager.keep_full_turns());
        assert!(compaction.compacted_turns <= 5);
    }
}
