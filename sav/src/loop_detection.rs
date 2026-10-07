use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Maximum number of recent actions retained in the sliding ring buffer.
pub const ACTION_HASH_RING_CAPACITY: usize = 8;

/// Number of consecutive identical actions that trigger a loop break.
pub const CONSECUTIVE_IDENTICAL_ACTION_LIMIT: usize = 2;

/// Number of occurrences of identical action with identical observation in the window that triggers a loop break.
pub const WINDOW_IDENTICAL_ACTION_LIMIT: usize = 3;

/// Number of consecutive identical observations across different actions that triggers an invariant loop break.
pub const OBSERVATION_INVARIANT_LIMIT: usize = 3;

/// Maximum consecutive stalled turns before triggering the hard circuit breaker.
pub const MAX_STALLED_TURNS: u32 = 4;

/// Default temperature jitter magnitude applied on Tier 2 recovery.
pub const DEFAULT_TEMPERATURE_JITTER: f32 = 0.5;

/// Threshold for reasoning n-gram Jaccard similarity indicating circular reasoning (0.88).
pub const REASONING_SIMILARITY_THRESHOLD: f64 = 0.88;

/// Default n-gram size for reasoning similarity evaluation (8).
pub const REASONING_NGRAM_SIZE: usize = 8;

/// Computes n-gram Jaccard similarity between two reasoning strings.
/// Text is tokenized by whitespace and ASCII punctuation, and n-grams of size `n`
/// are compared using set intersection over union.
pub fn compute_ngram_jaccard_similarity(text_a: &str, text_b: &str, n: usize) -> f64 {
    let tokenize = |text: &str| -> Vec<String> {
        text.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect()
    };

    let tokens_a = tokenize(text_a);
    let tokens_b = tokenize(text_b);

    if tokens_a.is_empty() && tokens_b.is_empty() {
        return 1.0;
    }
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }

    let n = n.max(1);
    if tokens_a.len() < n || tokens_b.len() < n {
        // When text is shorter than n tokens, compare word sets directly
        let set_a: std::collections::HashSet<&String> = tokens_a.iter().collect();
        let set_b: std::collections::HashSet<&String> = tokens_b.iter().collect();
        let intersection = set_a.intersection(&set_b).count();
        let union = set_a.union(&set_b).count();
        if union == 0 {
            return 1.0;
        }
        return intersection as f64 / union as f64;
    }

    let set_a: std::collections::HashSet<Vec<&str>> = tokens_a
        .windows(n)
        .map(|w| w.iter().map(|s| s.as_str()).collect())
        .collect();
    let set_b: std::collections::HashSet<Vec<&str>> = tokens_b
        .windows(n)
        .map(|w| w.iter().map(|s| s.as_str()).collect())
        .collect();

    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 1.0;
    }
    intersection as f64 / union as f64
}

/// Normalizes a JSON value into a canonical deterministic string representation.
/// Keys in objects are recursively sorted lexicographically.
pub fn normalize_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => {
            if *b {
                "true".to_owned()
            } else {
                "false".to_owned()
            }
        }
        Value::Number(num) => num.to_string(),
        Value::String(s) => serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\"")),
        Value::Array(arr) => {
            let mut out = String::from("[");
            for (i, elem) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&normalize_json(elem));
            }
            out.push(']');
            out
        }
        Value::Object(map) => {
            let mut sorted_keys: Vec<&String> = map.keys().collect();
            sorted_keys.sort();
            let mut out = String::from("{");
            for (i, key) in sorted_keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| format!("\"{key}\"")));
                out.push(':');
                out.push_str(&normalize_json(&map[*key]));
            }
            out.push('}');
            out
        }
    }
}

/// Computes the canonical SHA-256 action hash:
/// H_action = SHA256(tool_name || normalize_json(arguments))
pub fn compute_action_hash(tool_name: &str, arguments: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tool_name.as_bytes());
    hasher.update(normalize_json(arguments).as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Computes the observation hash:
/// H_obs = SHA256(stdout || \0 || stderr)
pub fn compute_observation_hash(stdout: &[u8], stderr: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(stdout);
    hasher.update(b"\0");
    hasher.update(stderr);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Record of an executed action and its resulting observation hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    /// Canonical hash of the action and its normalized arguments.
    pub action_hash: String,
    /// Tool name that was proposed.
    pub tool_name: String,
    /// Observation hash produced by executing the action, if completed.
    pub observation_hash: Option<String>,
}

/// Result of evaluating proposed actions or observations against the loop detector.
#[derive(Debug, Clone, PartialEq)]
pub enum LoopDecision {
    /// No loop detected; proceed with execution.
    Proceed,
    /// Tier 1: Soft correction rejecting the repetitive action without crashing.
    SoftCorrection {
        rejection_message: String,
        consecutive_stalls: u32,
    },
    /// Tier 2: Temperature jitter correction instructing dynamic elevation of inference temperature.
    TemperatureJitter {
        base_temperature: f32,
        jittered_temperature: f32,
        rejection_message: String,
        consecutive_stalls: u32,
    },
    /// Tier 3: Hard circuit breaker after max_stalled_turns reached.
    CircuitBreaker { reason: String, stalled_turns: u32 },
}

impl LoopDecision {
    /// Returns true if a loop break (soft correction, jitter, or circuit breaker) was triggered.
    pub fn is_loop_break(&self) -> bool {
        !matches!(self, Self::Proceed)
    }

    /// Returns the rejection message if this decision provides one.
    pub fn rejection_message(&self) -> Option<&str> {
        match self {
            Self::SoftCorrection {
                rejection_message, ..
            }
            | Self::TemperatureJitter {
                rejection_message, ..
            } => Some(rejection_message),
            _ => None,
        }
    }
}

/// Sliding ring buffer of recent actions for multi-tier loop detection.
#[derive(Debug, Clone)]
pub struct ActionHashRing {
    capacity: usize,
    history: VecDeque<ActionRecord>,
    recent_reasonings: VecDeque<String>,
    consecutive_stalls: u32,
    base_temperature: f32,
}

impl Default for ActionHashRing {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionHashRing {
    /// Creates a new action hash ring with default capacity (8) and base temperature (0.2).
    pub fn new() -> Self {
        Self::with_capacity(ACTION_HASH_RING_CAPACITY)
    }

    /// Creates a new action hash ring with specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            capacity: cap,
            history: VecDeque::with_capacity(cap),
            recent_reasonings: VecDeque::with_capacity(cap),
            consecutive_stalls: 0,
            base_temperature: 0.2,
        }
    }

    /// Sets the base inference temperature for temperature jitter calculations.
    pub fn set_base_temperature(&mut self, temp: f32) {
        self.base_temperature = temp;
    }

    /// Returns the number of consecutive stalls observed.
    pub fn consecutive_stalls(&self) -> u32 {
        self.consecutive_stalls
    }

    /// Resets consecutive stall counter.
    pub fn reset_stalls(&mut self) {
        self.consecutive_stalls = 0;
    }

    /// Evaluates a model reasoning trace against recent turns (Tier 3 N-gram similarity).
    pub fn check_reasoning(&mut self, reasoning: &str) -> LoopDecision {
        if let Some(last) = self.recent_reasonings.back() {
            let similarity =
                compute_ngram_jaccard_similarity(last, reasoning, REASONING_NGRAM_SIZE);
            if similarity >= REASONING_SIMILARITY_THRESHOLD {
                self.consecutive_stalls += 1;
                return self.formulate_decision(
                    2,
                    "The model reasoning trace indicates a circular rationale loop",
                );
            }
        }

        if self.recent_reasonings.len() >= self.capacity {
            self.recent_reasonings.pop_front();
        }
        self.recent_reasonings.push_back(reasoning.to_owned());
        LoopDecision::Proceed
    }

    /// Evaluates a proposed action against history before execution.
    pub fn check_proposed_action(&mut self, proposed_action_hash: &str) -> LoopDecision {
        let is_consecutive_repeat = self
            .history
            .back()
            .is_some_and(|last| last.action_hash == proposed_action_hash);

        let window_repeat_count = self
            .history
            .iter()
            .filter(|record| record.action_hash == proposed_action_hash)
            .count();

        let triggers_loop =
            is_consecutive_repeat || (window_repeat_count + 1 >= WINDOW_IDENTICAL_ACTION_LIMIT);

        if triggers_loop {
            self.consecutive_stalls += 1;
            self.formulate_decision(
                window_repeat_count + 1,
                "You have executed this exact action or reached an identical state",
            )
        } else {
            LoopDecision::Proceed
        }
    }

    /// Records an executed action into the ring buffer.
    pub fn record_action(&mut self, action_hash: String, tool_name: String) {
        if self.history.len() >= self.capacity {
            self.history.pop_front();
        }
        self.history.push_back(ActionRecord {
            action_hash,
            tool_name,
            observation_hash: None,
        });
    }

    /// Records the resulting observation hash for the most recent action and checks Tier 2 invariants.
    pub fn record_observation(&mut self, observation_hash: String) -> LoopDecision {
        if let Some(last) = self.history.back_mut() {
            last.observation_hash = Some(observation_hash.clone());
        }

        // Tier 2: Output invariant analysis.
        // If the last 3 actions produced the identical observation hash, state is stalled.
        if self.history.len() >= OBSERVATION_INVARIANT_LIMIT {
            let tail_obs: Vec<&Option<String>> = self
                .history
                .iter()
                .rev()
                .take(OBSERVATION_INVARIANT_LIMIT)
                .map(|rec| &rec.observation_hash)
                .collect();

            let all_match = tail_obs
                .iter()
                .all(|obs| obs.as_ref().is_some_and(|hash| hash == &observation_hash));

            if all_match {
                self.consecutive_stalls += 1;
                return self.formulate_decision(
                    OBSERVATION_INVARIANT_LIMIT,
                    "The environment observation did not change across recent actions",
                );
            }
        }

        // If progress mutated state, reset stalls.
        self.consecutive_stalls = 0;
        LoopDecision::Proceed
    }

    /// Formulates the progressive intervention decision (Tier 1 -> Tier 2 -> Tier 3).
    fn formulate_decision(&self, repeat_count: usize, detail: &str) -> LoopDecision {
        if self.consecutive_stalls >= MAX_STALLED_TURNS {
            return LoopDecision::CircuitBreaker {
                reason: format!(
                    "hard circuit breaker triggered after {} consecutive stalled turns: {}",
                    self.consecutive_stalls, detail
                ),
                stalled_turns: self.consecutive_stalls,
            };
        }

        let rejection_message = format!(
            "[SYSTEM REJECTION]: You have executed this exact action or reached an identical state {} times. \
             The environment did not change. You MUST choose a completely different approach, inspect alternative files, or terminate.",
            repeat_count.max(2)
        );

        if self.consecutive_stalls >= 2 {
            // Tier 2: Temperature Jitter
            let jittered = (self.base_temperature + DEFAULT_TEMPERATURE_JITTER).min(2.0);
            LoopDecision::TemperatureJitter {
                base_temperature: self.base_temperature,
                jittered_temperature: jittered,
                rejection_message,
                consecutive_stalls: self.consecutive_stalls,
            }
        } else {
            // Tier 1: Soft Correction
            LoopDecision::SoftCorrection {
                rejection_message,
                consecutive_stalls: self.consecutive_stalls,
            }
        }
    }

    /// Returns a slice of recent action records in chronological order.
    pub fn history(&self) -> Vec<&ActionRecord> {
        self.history.iter().collect()
    }

    /// Clears the history and resets stalls.
    pub fn clear(&mut self) {
        self.history.clear();
        self.recent_reasonings.clear();
        self.consecutive_stalls = 0;
    }
}
