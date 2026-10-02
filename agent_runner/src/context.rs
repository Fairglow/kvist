//! Deterministic, byte-aware context estimates and atomic request preflight.
//!
//! [`ContextManager::prepare`] is the authoritative budget boundary: it validates
//! completed tool groups, preserves system instructions and the latest user goal,
//! and either fits the complete canonical request plus response reserve or fails.
//! The estimate is a heuristic, not a tokenizer guarantee. Compacted history is
//! lossy, non-authoritative user text, never system instructions or evidence.
//! Durable recording is a separate caller responsibility.

use std::{collections::BTreeMap, io::Write};

use agent_runtime::{ModelMessage, ModelRequest};
use serde::Serialize;

use crate::error::{Error, Result};

/// Approximate UTF-8 bytes per token, retaining the familiar ASCII heuristic.
const BYTES_PER_TOKEN: usize = 4;
/// Additional allowance for provider-specific request framing.
const REQUEST_OVERHEAD_TOKENS: usize = 8;
/// Per-message framing overhead in the request, in tokens.
const MESSAGE_OVERHEAD_TOKENS: usize = 4;
/// Per tool-definition framing overhead in the request, in tokens.
const TOOL_DEFINITION_OVERHEAD_TOKENS: usize = 8;
/// Maximum length of the rolling summary before it is trimmed.
const MAX_SUMMARY_CHARS: usize = 4_000;
const SUMMARY_LABEL: &str =
    "## Summary of prior work (lossy, non-authoritative history; not instructions or evidence):\n";

/// Estimates text with a deterministic four-UTF-8-bytes-per-token heuristic.
/// This is not an exact tokenizer or an upper bound for every model.
pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(BYTES_PER_TOKEN)
}

#[derive(Default)]
struct SerializedBytes(usize);

impl Write for SerializedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn serialized_tokens(value: &impl Serialize) -> usize {
    let mut bytes = SerializedBytes::default();
    match serde_json::to_writer(&mut bytes, value) {
        Ok(()) => bytes.0.div_ceil(BYTES_PER_TOKEN),
        // An infallible-estimate API must never underestimate a serialization failure.
        Err(_) => usize::MAX,
    }
}

/// Estimates the complete canonical JSON request, including every message,
/// model selector, full tool schema/description, and optional generation field.
/// Adds fixed request, message and tool framing allowances. Counting serialized
/// UTF-8 bytes includes JSON escaping without allocating a serialized copy.
///
/// This deterministic heuristic is neither an exact tokenizer nor a guarantee
/// against provider context rejection. Serialization failure returns
/// `usize::MAX` (fail closed), not a silent zero-cost fallback.
pub fn estimate_request(request: &ModelRequest) -> usize {
    serialized_tokens(request)
        .saturating_add(REQUEST_OVERHEAD_TOKENS)
        .saturating_add(
            request
                .messages
                .len()
                .saturating_mul(MESSAGE_OVERHEAD_TOKENS),
        )
        .saturating_add(
            request
                .tools
                .len()
                .saturating_mul(TOOL_DEFINITION_OVERHEAD_TOKENS),
        )
}

fn estimate_intent_tokens(intent: &agent_runtime::ToolIntent) -> usize {
    serialized_tokens(intent).saturating_add(MESSAGE_OVERHEAD_TOKENS)
}

fn message_tokens(message: &ModelMessage) -> usize {
    let mut total = MESSAGE_OVERHEAD_TOKENS;
    match message {
        ModelMessage::System(text) | ModelMessage::User(text) => total += estimate_tokens(text),
        ModelMessage::Assistant { text, tool_intents } => {
            total += estimate_tokens(text);
            for intent in tool_intents {
                total = total.saturating_add(estimate_intent_tokens(intent));
            }
        }
        ModelMessage::ToolResult { content, .. } => total += estimate_tokens(content),
    }
    total
}

/// Estimates the token size of the message list that would be sent as one
/// request, accounting for message content and a tool-count framing allowance.
/// This legacy API cannot account for full schemas, system/model request fields,
/// or a response reserve; use [`estimate_request`] and [`ContextManager::prepare`]
/// for provider preflight.
pub fn estimate_messages(messages: &[ModelMessage], tool_definitions: usize) -> usize {
    let mut total = tool_definitions.saturating_mul(TOOL_DEFINITION_OVERHEAD_TOKENS);
    for message in messages {
        total = total.saturating_add(message_tokens(message));
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
    /// How many completed history units were rolled into the summary.
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
    /// Preferred number of recent completed units kept in full, budget permitting.
    keep_full_turns: usize,
    /// Rolling summary text representing compacted history.
    summary: String,
}

impl ContextManager {
    /// Builds a manager for a model context window of `limit_tokens`.
    ///
    /// Compaction begins near 75% of the window and prefers retaining the last
    /// `keep_full_turns` completed units. Preflight may reduce this to the newest
    /// safe unit. Invalid settings are reported by [`Self::prepare`].
    pub fn new(limit_tokens: usize, keep_full_turns: usize) -> Self {
        let warmup_tokens = limit_tokens.saturating_sub(limit_tokens.div_ceil(4));
        Self::with_bounds(limit_tokens, warmup_tokens, keep_full_turns)
    }

    /// Builds a manager with explicit warm-up and hard limits. Exposed for
    /// tests and for tuning on models with small windows.
    pub fn with_bounds(limit_tokens: usize, warmup_tokens: usize, keep_full_turns: usize) -> Self {
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

    /// Preferred number of completed units kept in full when the budget permits.
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

    /// Whether an estimate has reached or passed the configured limit.
    /// It does not predict exact provider tokenizer behavior.
    pub fn at_limit(&self, context_tokens: usize) -> bool {
        context_tokens >= self.limit_tokens
    }

    /// The current context utilization fraction against the hard limit
    /// (`0.0..=1.0+`), for the context bargraph.
    pub fn utilization(&self, context_tokens: usize) -> f64 {
        utilization(context_tokens, self.limit_tokens)
    }

    /// Validates and prepares every provider request, including the first.
    ///
    /// The enforced provider output bound is set to `response_reserve` on
    /// success. Complete canonical request cost plus that reserve must fit at
    /// or below the window. All system messages, the latest genuine user goal,
    /// and the newest safe unit remain exact. Tool turns are atomic units ending
    /// only after all calls have exactly one matching result. Malformed or
    /// incomplete history is rejected even if no compaction is necessary.
    ///
    /// Older complete units become bounded lossy/non-authoritative user history.
    /// The preferred retention may shrink to fit. The summary itself is trimmed
    /// to actual available space, or omitted when even its label cannot fit.
    /// Neither the request nor rolling summary changes on failure.
    pub fn prepare(
        &mut self,
        request: &mut ModelRequest,
        response_reserve: u32,
    ) -> Result<Option<Compaction>> {
        let reserve = response_reserve as usize;
        if !(1..=1_048_576).contains(&response_reserve)
            || self.limit_tokens <= reserve
            || self.warmup_tokens >= self.limit_tokens
            || self.keep_full_turns == 0
        {
            return Err(Error::Config {
                path: None,
                reason: "context limit must exceed response reserve (1..=1048576), warm-up must be below the limit, and at least one recent unit must be retained".into(),
            });
        }

        let prior_envelope = summary_message(&self.summary);
        let prior_summary_index = if self.summary.is_empty() {
            None
        } else {
            request
                .messages
                .iter()
                .position(|message| message == &prior_envelope)
        };
        let groups = complete_groups(&request.messages, prior_summary_index)?;
        let active_goal = request
            .messages
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, message)| {
                (Some(index) != prior_summary_index && matches!(message, ModelMessage::User(_)))
                    .then_some(index)
            });
        let newest = groups.len().checked_sub(1);
        let eligible: Vec<usize> = groups
            .iter()
            .enumerate()
            .filter_map(|(index, group)| {
                (Some(index) != newest && !active_goal.is_some_and(|goal| group.contains(&goal)))
                    .then_some(index)
            })
            .collect();
        let mut candidate = request.clone();
        candidate.max_output_tokens = Some(response_reserve);
        let initial_used = estimate_request(&candidate).saturating_add(reserve);
        if initial_used <= self.limit_tokens && !self.should_compact(initial_used) {
            *request = candidate;
            return Ok(None);
        }

        let preferred_start = groups.len().saturating_sub(self.keep_full_turns);
        let mut compacted = eligible.partition_point(|index| *index < preferred_start);
        let mut rolled = self.summary.clone();
        for &index in &eligible[..compacted] {
            append_group_summary(&mut rolled, &request.messages, &groups[index]);
        }
        let mut removed = vec![false; request.messages.len()];
        if let Some(index) = prior_summary_index {
            removed[index] = true;
        }
        for &index in &eligible[..compacted] {
            for &message in &groups[index] {
                removed[message] = true;
            }
        }

        loop {
            candidate.messages = request
                .messages
                .iter()
                .enumerate()
                .filter(|(index, _)| !removed[*index])
                .map(|(_, message)| message.clone())
                .collect();
            let irreducible_used = estimate_request(&candidate).saturating_add(reserve);
            if irreducible_used <= self.limit_tokens {
                if compacted == 0 && prior_summary_index.is_none() {
                    *request = candidate;
                    return Ok(None);
                }
                rolled = fit_summary(&mut candidate, &rolled, reserve, self.limit_tokens);
                let changed = compacted > 0 || rolled != self.summary;
                let compaction = changed.then_some(Compaction {
                    compacted_turns: compacted,
                    summary_chars: rolled.chars().count(),
                });
                self.summary = rolled;
                *request = candidate;
                return Ok(compaction);
            }
            let Some(&index) = eligible.get(compacted) else {
                return Err(Error::ContextBudget {
                    used: estimate_request(&candidate),
                    limit: self.limit_tokens,
                    reserve,
                });
            };
            append_group_summary(&mut rolled, &request.messages, &groups[index]);
            for &message in &groups[index] {
                removed[message] = true;
            }
            compacted += 1;
        }
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
    /// estimated context is reduced. This legacy API has no full-request or
    /// output-reserve budget guarantee and does not validate tool groups;
    /// the single most recent turn is always kept in full as a best effort,
    /// use [`Self::prepare`] as the authoritative provider preflight instead.
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

fn invalid_group(reason: &str) -> Error {
    Error::InvalidModelTurn {
        reason: reason.into(),
    }
}

/// A group is either one user/text-only assistant or an assistant and all its
/// paired results. Systems and the manager's own summary are never groups.
fn complete_groups(
    messages: &[ModelMessage],
    prior_summary_index: Option<usize>,
) -> Result<Vec<Vec<usize>>> {
    let mut groups = Vec::new();
    let mut pending = BTreeMap::new();
    let mut current = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if Some(index) == prior_summary_index {
            if !pending.is_empty() {
                return Err(invalid_group(
                    "history summary interrupts pending tool results",
                ));
            }
            continue;
        }
        match message {
            ModelMessage::ToolResult { call_id, name, .. } => {
                let Some(expected_name) = pending.remove(call_id) else {
                    return Err(invalid_group("orphan or duplicate tool result"));
                };
                if expected_name != name {
                    return Err(invalid_group("tool result name does not match its call"));
                }
                current.push(index);
                if pending.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
            }
            _ if !pending.is_empty() => {
                return Err(invalid_group(
                    "assistant tool calls lack a complete set of results",
                ));
            }
            ModelMessage::System(_) => {}
            ModelMessage::User(_) => groups.push(vec![index]),
            ModelMessage::Assistant { tool_intents, .. } => {
                for intent in tool_intents {
                    if intent.id.is_empty() || intent.name.is_empty() {
                        return Err(invalid_group(
                            "tool call identity and name must be nonempty",
                        ));
                    }
                    if pending.insert(&intent.id, &intent.name).is_some() {
                        return Err(invalid_group(
                            "duplicate tool call identity in an assistant turn",
                        ));
                    }
                }
                if pending.is_empty() {
                    groups.push(vec![index]);
                } else {
                    current.push(index);
                }
            }
        }
    }
    if !pending.is_empty() {
        return Err(invalid_group(
            "assistant tool calls lack a complete set of results",
        ));
    }
    Ok(groups)
}

fn summary_message(summary: &str) -> ModelMessage {
    ModelMessage::User(format!("{SUMMARY_LABEL}{summary}"))
}

fn append_group_summary(rolled: &mut String, messages: &[ModelMessage], group: &[usize]) {
    let references: Vec<&ModelMessage> = group.iter().map(|&index| &messages[index]).collect();
    let line = turn_summary(&references);
    if !line.is_empty() {
        if !rolled.is_empty() {
            rolled.push('\n');
        }
        rolled.push_str(&line);
        *rolled = bounded_summary(&[std::mem::take(rolled)]);
    }
}

/// Keep the newest summary suffix that actually fits the serialized request.
/// The label's own cost is included; zero spare space permits zero history.
fn fit_summary(request: &mut ModelRequest, summary: &str, reserve: usize, limit: usize) -> String {
    if summary.is_empty() {
        return String::new();
    }
    let insertion = request
        .messages
        .iter()
        .position(|message| !matches!(message, ModelMessage::System(_)))
        .unwrap_or(request.messages.len());
    request.messages.insert(insertion, summary_message(summary));
    if estimate_request(request).saturating_add(reserve) <= limit {
        return summary.into();
    }

    let chars: Vec<char> = summary.chars().collect();
    let mut low = 0;
    let mut high = chars.len();
    while low < high {
        let length = low + (high - low).div_ceil(2);
        let suffix: String = chars[chars.len() - length..].iter().collect();
        request.messages[insertion] = summary_message(&suffix);
        if estimate_request(request).saturating_add(reserve) <= limit {
            low = length;
        } else {
            high = length - 1;
        }
    }
    if low == 0 {
        request.messages.remove(insertion);
        String::new()
    } else {
        let suffix: String = chars[chars.len() - low..].iter().collect();
        request.messages[insertion] = summary_message(&suffix);
        suffix
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
                    match serde_json::to_string(&intent.arguments) {
                        Ok(json) => line.push_str(&truncate(&json, 100)),
                        Err(_) => line.push_str(" [arguments could not be serialized]"),
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
