use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

/// A discrete recorded event in an agent execution trajectory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TrajectoryEvent {
    /// Initialization of an autonomous agent execution session.
    SessionStart {
        session_id: String,
        task_id: String,
        timestamp: u64,
    },
    /// Beginning of a Reason-Act-Observe turn.
    TurnStart { turn: usize, timestamp: u64 },
    /// Model prompt evaluation and token consumption metrics.
    PromptEval {
        turn: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cached_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_tokens: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        eval_duration_ms: Option<u64>,
    },
    /// Model internal reasoning or thinking trace before tool selection.
    ModelReasoning { turn: usize, reasoning: String },
    /// Dispatch of an approved tool call to the sandbox environment.
    ToolDispatch {
        turn: usize,
        call_id: String,
        tool: String,
        args: Value,
        action_hash: String,
    },
    /// Observation resulting from tool execution inside the sandbox.
    ToolResult {
        turn: usize,
        call_id: String,
        tool: String,
        stdout: String,
        stderr: String,
        exit_code: i32,
        bytes: usize,
        state_mutated: bool,
    },
    /// Completion of a single agent turn.
    TurnFinish {
        turn: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u64>,
        finish_reason: String,
    },
    /// Terminal disposition of the entire session.
    SessionFinish {
        session_id: String,
        task_id: String,
        total_turns: usize,
        total_tokens: u64,
        success: bool,
    },
}

/// Appends structured trajectory events to a `.jsonl` journal file.
#[derive(Debug, Clone)]
pub struct TrajectoryRecorder {
    path: PathBuf,
}

impl TrajectoryRecorder {
    /// Creates a recorder configured to write events to the specified file path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Appends one event to the journal file.
    pub fn record_event(&self, event: &TrajectoryEvent) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|source| Error::Io {
                operation: "open trajectory journal file",
                path: self.path.clone(),
                source,
            })?;
        let json = serde_json::to_string(event).map_err(|e| Error::InvalidModelRequest {
            reason: format!("failed to serialize trajectory event: {e}"),
        })?;
        writeln!(file, "{json}").map_err(|source| Error::Io {
            operation: "write trajectory journal event",
            path: self.path.clone(),
            source,
        })?;
        Ok(())
    }

    /// Returns the target path of the trajectory journal.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Summary report produced by replaying a historical session trajectory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrajectoryReplayReport {
    /// Identifier of the replayed session.
    pub session_id: Option<String>,
    /// Task identifier of the session.
    pub task_id: Option<String>,
    /// Highest turn index reached during replay.
    pub replayed_turns: usize,
    /// Total events parsed and replayed.
    pub total_events: usize,
    /// Number of tool invocations replayed.
    pub tool_calls_count: usize,
    /// Final outcome recorded in the session, if present.
    pub final_success: Option<bool>,
    /// Ordered list of replayed events.
    pub events: Vec<TrajectoryEvent>,
}

/// Replays a structured `.jsonl` session trajectory up to an optional turn ceiling.
pub fn replay_trajectory(path: &Path, max_turns: Option<usize>) -> Result<TrajectoryReplayReport> {
    let file = File::open(path).map_err(|source| Error::Io {
        operation: "open trajectory journal for replay",
        path: path.to_path_buf(),
        source,
    })?;
    let reader = BufReader::new(file);

    let mut session_id = None;
    let mut task_id = None;
    let mut current_turn = 0;
    let mut tool_calls_count = 0;
    let mut final_success = None;
    let mut filtered_events = Vec::new();

    for line_res in reader.lines() {
        let line = line_res.map_err(|source| Error::Io {
            operation: "read trajectory journal line",
            path: path.to_path_buf(),
            source,
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event: TrajectoryEvent =
            serde_json::from_str(trimmed).map_err(|e| Error::MalformedModelResponse {
                reason: format!("malformed trajectory journal line: {e}"),
            })?;

        match &event {
            TrajectoryEvent::SessionStart {
                session_id: s,
                task_id: t,
                ..
            } => {
                session_id = Some(s.clone());
                task_id = Some(t.clone());
            }
            TrajectoryEvent::TurnStart { turn, .. } => {
                current_turn = *turn;
            }
            TrajectoryEvent::ToolDispatch { .. } => {
                tool_calls_count += 1;
            }
            TrajectoryEvent::SessionFinish { success, .. } => {
                final_success = Some(*success);
            }
            _ => {}
        }

        if let Some(limit) = max_turns
            && current_turn > limit
        {
            break;
        }

        filtered_events.push(event);
    }

    let highest_turn = filtered_events
        .iter()
        .filter_map(|e| match e {
            TrajectoryEvent::TurnStart { turn, .. }
            | TrajectoryEvent::PromptEval { turn, .. }
            | TrajectoryEvent::ModelReasoning { turn, .. }
            | TrajectoryEvent::ToolDispatch { turn, .. }
            | TrajectoryEvent::ToolResult { turn, .. }
            | TrajectoryEvent::TurnFinish { turn, .. } => Some(*turn),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    Ok(TrajectoryReplayReport {
        session_id,
        task_id,
        replayed_turns: highest_turn,
        total_events: filtered_events.len(),
        tool_calls_count,
        final_success,
        events: filtered_events,
    })
}
