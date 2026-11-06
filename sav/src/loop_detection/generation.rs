use std::collections::VecDeque;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{FinishReason, ModelStreamEvent, ModelTurn};

const HISTORY: usize = 2048;
const MAX_PERIOD: usize = 512;
const WORD_CADENCE: u64 = 8;
const CHAR_CADENCE: usize = 128;
const MIN_UNBROKEN_CHARS: usize = 1024;
const MAX_NEAR_CANDIDATES: usize = 16;
const BASE: u64 = 0x9e37_79b1_85eb_ca87;

/// Independent provider-output channel containing repetition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepetitionChannel {
    Answer,
    Reasoning,
}

/// Units of the observed periodic passage; Unicode scalars are characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepetitionUnit {
    Words,
    Characters,
}

/// Bounded detection metadata, never the untrusted repeated text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepetitionReport {
    pub channel: RepetitionChannel,
    pub unit: RepetitionUnit,
    pub period: usize,
    pub copies: usize,
    pub similarity_percent: u8,
}

impl fmt::Display for RepetitionReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let channel = match self.channel {
            RepetitionChannel::Answer => "answer",
            RepetitionChannel::Reasoning => "reasoning",
        };
        let unit = match self.unit {
            RepetitionUnit::Words => "word",
            RepetitionUnit::Characters => "character",
        };
        write!(
            formatter,
            "{channel}: approximately {}-{unit} period, {} copies, {}% identity",
            self.period, self.copies, self.similarity_percent,
        )
    }
}

impl RepetitionReport {
    /// Goal-preserving corrective instructions containing only trusted metadata.
    pub fn feedback(self, step: RecoveryStep) -> String {
        format!(
            "Detected repetitive generation ({self}). Recovery {}/{}: the incomplete response was discarded and none of its tools ran; earlier completed work is preserved. Keep the original goal and approved authority. Stop repeating this passage. Choose a materially different supported strategy or a useful allowed inspection, give a supported final answer, or explain the concrete blocker. Do not invent progress, make unnecessary edits, change sampling to escape, or replay uncertain effects.",
            step.attempt, step.limit,
        )
    }

    /// Honest stopping guidance after the finite correction allowance is spent.
    pub fn exhausted_message(self) -> String {
        format!(
            "generation stalled: repetitive output persisted after two autonomous corrections ({self}). No tools from discarded responses ran; earlier completed work is preserved. Inspect the current state and transcript, resolve the concrete blocker or adjust the task/model, then submit a new prompt. Intentional repeated output requires an explicit host exception."
        )
    }
}

/// One genuine autonomous correction opportunity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryStep {
    pub attempt: u8,
    pub limit: u8,
}

/// Prompt/authoring-sequence recovery accounting, outside compactable messages.
#[derive(Debug, Default)]
pub struct RecoveryBudget {
    spent: u8,
}

impl RecoveryBudget {
    /// Consumes one of two corrections. There is deliberately no reset method.
    pub fn take(&mut self) -> Option<RecoveryStep> {
        if self.spent == 2 {
            return None;
        }
        self.spent += 1;
        Some(RecoveryStep {
            attempt: self.spent,
            limit: 2,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct Pattern {
    unit: RepetitionUnit,
    period: usize,
    copies: usize,
    similarity_percent: u8,
}

impl Pattern {
    fn report(self, channel: RepetitionChannel) -> RepetitionReport {
        RepetitionReport {
            channel,
            unit: self.unit,
            period: self.period,
            copies: self.copies,
            similarity_percent: self.similarity_percent,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Word {
    fingerprint: u128,
    prose: bool,
    numeric: bool,
}

impl Word {
    fn signature(self) -> u64 {
        self.fingerprint as u64
            ^ (self.fingerprint >> 64) as u64
            ^ u64::from(self.prose)
            ^ (u64::from(self.numeric) << 1)
    }
}

struct Signatures {
    prefix: Vec<u64>,
    powers: Vec<u64>,
    structured: Vec<usize>,
}

impl Signatures {
    fn new(values: impl ExactSizeIterator<Item = (u64, bool)>) -> Self {
        let mut prefix = Vec::with_capacity(values.len() + 1);
        let mut powers = Vec::with_capacity(values.len() + 1);
        let mut structured = Vec::with_capacity(values.len() + 1);
        let (mut hash, mut power, mut count) = (0_u64, 1_u64, 0_usize);
        prefix.push(hash);
        powers.push(power);
        structured.push(count);
        for (value, prose) in values {
            hash = hash.wrapping_mul(BASE).wrapping_add(value);
            power = power.wrapping_mul(BASE);
            count += usize::from(!prose);
            prefix.push(hash);
            powers.push(power);
            structured.push(count);
        }
        Self {
            prefix,
            powers,
            structured,
        }
    }

    fn hash(&self, start: usize, len: usize) -> u64 {
        self.prefix[start + len].wrapping_sub(self.prefix[start].wrapping_mul(self.powers[len]))
    }

    fn prose(&self, start: usize, len: usize) -> bool {
        self.structured[start] == self.structured[start + len]
    }
}

fn exact_period<T: Eq>(
    values: &[T],
    signatures: &Signatures,
    copies: impl Fn(usize) -> usize,
) -> Option<(usize, usize)> {
    for period in 1..=MAX_PERIOD.min(values.len() / 2) {
        let copies = copies(period);
        let span = period * copies;
        if span > values.len() {
            continue;
        }
        let start = values.len() - span;
        let first = &values[start..start + period];
        let signature = signatures.hash(start, period);
        if (1..copies).all(|copy| {
            let next = start + copy * period;
            signatures.hash(next, period) == signature && first == &values[next..next + period]
        }) {
            return Some((period, copies));
        }
    }
    None
}

fn near_period(values: &[Word], signatures: &Signatures, terminal: bool) -> Option<Pattern> {
    let mut candidates = 0;
    for period in 50..=MAX_PERIOD.min(values.len() / 3) {
        let start = values.len() - 3 * period;
        if !signatures.prose(start, 3 * period) {
            continue;
        }
        let max_mismatches = period * 6 / 100;
        let matches_anchor = |offset| {
            let first = signatures.hash(start + offset, 8);
            first == signatures.hash(start + period + offset, 8)
                && first == signatures.hash(start + 2 * period + offset, 8)
        };
        let anchor_matches = if terminal {
            let anchors = max_mismatches * 3 / 2 + 1;
            (0..anchors).any(|index| matches_anchor(index * (period - 8) / (anchors - 1)))
        } else {
            [0, period / 2, period - 8].into_iter().any(matches_anchor)
        };
        if !anchor_matches {
            continue;
        }
        candidates += 1;
        let first = &values[start..start + period];
        let second = &values[start + period..start + 2 * period];
        let third = &values[start + 2 * period..];
        let mut worst = 0;
        let similar = [(first, second), (first, third), (second, third)]
            .into_iter()
            .all(|(left, right)| {
                let mut mismatches = 0;
                for (a, b) in left.iter().zip(right) {
                    if a != b {
                        if a.numeric || b.numeric {
                            return false;
                        }
                        mismatches += 1;
                        if mismatches > max_mismatches {
                            return false;
                        }
                    }
                }
                worst = worst.max(mismatches);
                true
            });
        let mut distinct = Vec::with_capacity(8);
        if similar {
            for word in first {
                if !distinct.contains(&word.fingerprint) {
                    distinct.push(word.fingerprint);
                    if distinct.len() == 8 {
                        return Some(Pattern {
                            unit: RepetitionUnit::Words,
                            period,
                            copies: 3,
                            similarity_percent: ((period - worst) * 100 / period) as u8,
                        });
                    }
                }
            }
        }
        if candidates == MAX_NEAR_CANDIDATES {
            break;
        }
    }
    None
}

fn prose_prefix(prefix: &str) -> bool {
    if prefix.starts_with([
        '#', '>', '|', '`', '"', '\'', '“', '‘', '{', '}', '[', ']', '<',
    ]) || prefix.starts_with("~~~")
        || matches!(prefix, "-" | "*" | "+")
    {
        return false;
    }
    !((prefix.ends_with('.') || prefix.ends_with(')'))
        && prefix[..prefix.len() - 1]
            .bytes()
            .all(|byte| byte.is_ascii_digit()))
}

struct ChannelDetector {
    words: VecDeque<Word>,
    scalars: VecDeque<u32>,
    hasher: Sha256,
    prefix: String,
    prefix_chars: u8,
    word_chars: usize,
    word_structured: bool,
    word_numeric: bool,
    marker: Option<(char, usize)>,
    word_inline_marker: bool,
    words_seen: u64,
    first_word: bool,
    line_prose: bool,
    indent: u8,
    fence: Option<(char, usize)>,
    pending_fence_close: bool,
    quote: Option<char>,
    inline_code: Option<usize>,
    pending_ticks: usize,
    detected: Option<Pattern>,
}

impl Default for ChannelDetector {
    fn default() -> Self {
        Self {
            words: VecDeque::with_capacity(HISTORY),
            scalars: VecDeque::with_capacity(HISTORY),
            hasher: Sha256::new(),
            prefix: String::new(),
            prefix_chars: 0,
            word_chars: 0,
            word_structured: false,
            word_numeric: false,
            marker: None,
            word_inline_marker: false,
            words_seen: 0,
            first_word: true,
            line_prose: true,
            indent: 0,
            fence: None,
            pending_fence_close: false,
            quote: None,
            inline_code: None,
            pending_ticks: 0,
            detected: None,
        }
    }
}

impl ChannelDetector {
    fn current_prose(&self) -> bool {
        self.line_prose
            && self.fence.is_none()
            && self.quote.is_none()
            && self.inline_code.is_none()
            && self.indent < 4
            && !self.word_structured
            && (!self.first_word || prose_prefix(&self.prefix))
    }

    fn flush_ticks(&mut self) {
        let count = std::mem::take(&mut self.pending_ticks);
        if count == 0 || self.fence.is_some() || self.quote.is_some() || self.indent >= 4 {
            return;
        }
        if self.inline_code == Some(count) {
            self.inline_code = None;
            self.word_inline_marker = true;
        } else if self.inline_code.is_none()
            && !(self.first_word && self.word_chars == count && count >= 3)
        {
            self.inline_code = Some(count);
            self.word_inline_marker = true;
        }
    }

    fn end_line(&mut self) {
        if self.pending_fence_close {
            self.fence = None;
        }
        self.pending_fence_close = false;
        self.first_word = true;
        self.indent = 0;
        self.line_prose =
            self.fence.is_none() && self.quote.is_none() && self.inline_code.is_none();
    }

    fn inspect_word(&mut self, length_warning: bool, terminal: bool) -> Option<Pattern> {
        let words = self.words.make_contiguous();
        let signatures = Signatures::new(words.iter().map(|word| (word.signature(), word.prose)));
        let exact = exact_period(words, &signatures, |period| {
            if period < 50 {
                6.max(48_usize.div_ceil(period))
            } else if length_warning
                && 2 * period <= words.len()
                && signatures.prose(words.len() - 2 * period, 2 * period)
            {
                2
            } else {
                3
            }
        });
        exact
            .map(|(period, copies)| Pattern {
                unit: RepetitionUnit::Words,
                period,
                copies,
                similarity_percent: 100,
            })
            .or_else(|| near_period(words, &signatures, terminal))
    }

    fn inspect_unbroken(&mut self) -> Option<Pattern> {
        if self.word_chars < MIN_UNBROKEN_CHARS || !self.current_prose() {
            return None;
        }
        let scalars = self.scalars.make_contiguous();
        let signatures = Signatures::new(scalars.iter().map(|value| (u64::from(*value), true)));
        exact_period(scalars, &signatures, |period| {
            3.max(MIN_UNBROKEN_CHARS.div_ceil(period))
        })
        .map(|(period, copies)| Pattern {
            unit: RepetitionUnit::Characters,
            period,
            copies,
            similarity_percent: 100,
        })
    }

    fn finish_word(&mut self) -> Option<Pattern> {
        if self.word_chars == 0 {
            return None;
        }
        self.flush_ticks();
        if let Some(pattern) = self.inspect_unbroken() {
            return Some(pattern);
        }
        if self.first_word {
            if let Some((marker, len)) = self.marker {
                match self.fence {
                    Some((open, min)) if open == marker && len >= min && len == self.word_chars => {
                        self.pending_fence_close = true;
                    }
                    None if len >= 3
                        && self.indent < 4
                        && self.quote.is_none()
                        && self.inline_code.is_none()
                        && !self.word_inline_marker =>
                    {
                        self.fence = Some((marker, len));
                    }
                    _ => {}
                }
            }
            self.line_prose = self.fence.is_none() && self.indent < 4 && prose_prefix(&self.prefix);
            self.first_word = false;
        }
        let digest = self.hasher.finalize_reset();
        let word = Word {
            fingerprint: u128::from_le_bytes(std::array::from_fn(|index| digest[index])),
            prose: self.current_prose(),
            numeric: self.word_numeric,
        };
        if self.words.len() == HISTORY {
            self.words.pop_front();
        }
        self.words.push_back(word);
        self.words_seen = self.words_seen.saturating_add(1);
        self.word_chars = 0;
        self.prefix_chars = 0;
        self.prefix.clear();
        self.word_structured = false;
        self.word_numeric = false;
        self.marker = None;
        self.word_inline_marker = false;
        self.scalars.clear();
        self.words_seen
            .is_multiple_of(WORD_CADENCE)
            .then(|| self.inspect_word(false, false))
            .flatten()
    }

    fn feed_while(&mut self, text: &str, keep_going: &mut dyn FnMut() -> bool) -> Option<Pattern> {
        if self.detected.is_some() {
            return self.detected;
        }
        for (index, ch) in text.chars().enumerate() {
            if index.is_multiple_of(256) && !keep_going() {
                return None;
            }
            if ch != '`' {
                self.flush_ticks();
            }
            if ch.is_whitespace() {
                if let Some(pattern) = self.finish_word() {
                    self.detected = Some(pattern);
                    return self.detected;
                }
                if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                    self.end_line();
                } else if self.first_word {
                    if ch == '\t' {
                        self.indent = 4;
                    } else if ch == ' ' {
                        self.indent = self.indent.saturating_add(1).min(4);
                    }
                }
                continue;
            }
            if self.word_chars == 0 {
                if !self.first_word {
                    self.pending_fence_close = false;
                }
                self.marker = matches!(ch, '`' | '~').then_some((ch, 1));
            } else if let Some((marker, len)) = self.marker.as_mut()
                && *marker == ch
                && *len == self.word_chars
            {
                *len = len.saturating_add(1);
            }
            if ch == '`' {
                self.pending_ticks = self.pending_ticks.saturating_add(1);
            }
            let quote_marker = self.quote == Some(ch)
                || matches!(ch, '"' | '“' | '‘')
                || (ch == '\'' && self.word_chars == 0);
            if self.fence.is_none() && self.inline_code.is_none() {
                if self.quote == Some(ch) {
                    self.quote = None;
                } else if self.quote.is_none() {
                    self.quote = match ch {
                        '"' => Some('"'),
                        '“' => Some('”'),
                        '‘' => Some('’'),
                        '\'' if self.word_chars == 0 => Some('\''),
                        _ => None,
                    };
                }
            }
            if self.prefix_chars < 8 {
                self.prefix.push(ch.to_ascii_lowercase());
                self.prefix_chars += 1;
            }
            self.word_structured |=
                quote_marker || matches!(ch, '`' | '{' | '}' | '|' | '[' | ']' | '<' | '>');
            self.word_numeric |= ch.is_numeric();
            self.word_chars = self.word_chars.saturating_add(1);
            let mut buffer = [0_u8; 4];
            for lower in ch.to_lowercase() {
                self.hasher
                    .update(lower.encode_utf8(&mut buffer).as_bytes());
            }
            if self.scalars.len() == HISTORY {
                self.scalars.pop_front();
            }
            self.scalars.push_back(ch.to_ascii_lowercase() as u32);
            if self.word_chars.is_multiple_of(CHAR_CADENCE)
                && let Some(pattern) = self.inspect_unbroken()
            {
                self.detected = Some(pattern);
                return self.detected;
            }
        }
        None
    }

    #[cfg(test)]
    fn feed(&mut self, text: &str) -> Option<Pattern> {
        self.feed_while(text, &mut || true)
    }

    fn finish(&mut self, length_warning: bool) -> Option<Pattern> {
        self.detected = self
            .detected
            .or_else(|| self.finish_word())
            .or_else(|| self.inspect_word(length_warning, true));
        self.detected
    }
}

/// Incremental, chunk-independent repetition inspection with bounded retention.
/// Answer and supplied reasoning are independent; tool payloads are never scanned.
#[derive(Default)]
pub struct StreamLoopDetector {
    answer: ChannelDetector,
    reasoning: ChannelDetector,
    interrupted: bool,
}

impl StreamLoopDetector {
    /// Inspects a decoded provisional event, before accepting any proposal.
    pub fn observe(&mut self, event: &ModelStreamEvent) -> Option<RepetitionReport> {
        self.observe_while(event, &mut || true)
    }

    /// Polls host controls within large events. A closed predicate is latched;
    /// the host must discard the attempt rather than interpret `None` as acceptance.
    pub fn observe_while(
        &mut self,
        event: &ModelStreamEvent,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> Option<RepetitionReport> {
        if self.interrupted || !keep_going() {
            self.interrupted = true;
            return None;
        }
        let mut interrupted = false;
        let mut check = || {
            let open = keep_going();
            interrupted |= !open;
            open
        };
        let report = match event {
            ModelStreamEvent::TextDelta(text) => self
                .answer
                .feed_while(text, &mut check)
                .map(|pattern| pattern.report(RepetitionChannel::Answer)),
            ModelStreamEvent::ReasoningDelta(text) => self
                .reasoning
                .feed_while(text, &mut check)
                .map(|pattern| pattern.report(RepetitionChannel::Reasoning)),
            ModelStreamEvent::ToolIntent(_) => None,
        };
        self.interrupted |= interrupted;
        report
    }

    /// Flushes final words and checks complete output even when events were omitted.
    /// A length-limited response also receives conservative two-copy prose checks.
    pub fn finish(&mut self, turn: &ModelTurn) -> Option<RepetitionReport> {
        self.finish_while(turn, &mut || true)
    }

    /// Control-responsive final/unary inspection with the same interruption latch.
    pub fn finish_while(
        &mut self,
        turn: &ModelTurn,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> Option<RepetitionReport> {
        if self.interrupted {
            return None;
        }
        let length = matches!(turn.finish_reason, FinishReason::Length);
        for (channel, detector, text) in [
            (
                RepetitionChannel::Answer,
                &mut self.answer,
                turn.text.as_str(),
            ),
            (
                RepetitionChannel::Reasoning,
                &mut self.reasoning,
                turn.reasoning.as_deref().unwrap_or_default(),
            ),
        ] {
            if !keep_going() {
                self.interrupted = true;
                return None;
            }
            if let Some(pattern) = detector.finish(length) {
                return Some(pattern.report(channel));
            }
            let mut complete = ChannelDetector::default();
            let mut interrupted = false;
            let pattern = complete.feed_while(text, &mut || {
                let open = keep_going();
                interrupted |= !open;
                open
            });
            if interrupted || !keep_going() {
                self.interrupted = true;
                return None;
            }
            if let Some(pattern) = pattern.or_else(|| complete.finish(length)) {
                return Some(pattern.report(channel));
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
