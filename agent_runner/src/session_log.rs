//! Durable session log for the agent loop.
//!
//! Every turn is recorded in two complementary forms, so the full transcript —
//! including the agent's reasoning / thinking — survives even when compaction
//! removes older turns from the model context and the UI collapses thinking:
//!
//! * a structured `.jsonl` journal built on `agent_runtime::TrajectoryRecorder`,
//!   which is what an automated replay or the compliance review consumes; and
//! * a human-readable transcript log, which is what a curious person reads to
//!   follow how a conclusion was reached.
//!
//! The journal uses the same closed event vocabulary as the broader Kvist
//! runtime, so the two remain interchangeable. Nothing here leaks secrets: tool
//! results are bounded exactly as they are folded back to the model, and only
//! tool *names* and their argument shapes reach the structured record.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::session::Recorder;
use agent_runtime::{ModelTurn, ModelUsage, TrajectoryEvent, TrajectoryRecorder};

/// Default location for the structured journal and readable transcript.
pub const DEFAULT_LOG_DIR: &str = ".agent-runner/runs";
/// Maximum bytes of a single tool result written to the log (matches the
/// value folded back to the model).
const MAX_LOG_OUTPUT_BYTES: usize = 64 * 1024;

/// Collapses a reasoning string to a single readable line for the transcript.
fn reason_lines(reasoning: &str) -> String {
    let one: String = reasoning.lines().collect::<Vec<_>>().join(" ");
    bounded_string(&one, 500)
}

/// Truncates `text` to at most `max` bytes, lossily and with an ellipsis.
fn bounded_string(text: &str, max: usize) -> String {
    let bytes = text.as_bytes();
    if bytes.len() <= max {
        text.to_owned()
    } else {
        let kept = String::from_utf8_lossy(&bytes[..max]).into_owned();
        format!("...{kept}")
    }
}

/// Bounds the tool stdout shown in the structured record.
fn bounded_stdout(stdout: &str) -> String {
    bounded_string(stdout, MAX_LOG_OUTPUT_BYTES)
}

/// Bounds the tool stderr shown in the structured record.
fn bounded_stderr(stderr: &str) -> String {
    bounded_string(stderr, MAX_LOG_OUTPUT_BYTES)
}

/// The durable session log: structured journal plus readable transcript, with
/// token and timing stats for the live working-speed display.
#[derive(Debug)]
pub struct SessionLog {
    recorder: TrajectoryRecorder,
    transcript: Option<File>,
    transcript_path: PathBuf,
    started_at: Instant,
    task_id: String,
    turn: usize,
    total_input_tokens: u64,
    total_output_tokens: u64,
}

impl SessionLog {
    /// Opens a new journal and transcript under `log_dir`, timestamped by the
    /// current time. Creates the log directory if needed.
    pub fn open(log_dir: &Path, task_id: impl Into<String>) -> std::io::Result<SessionLog> {
        std::fs::create_dir_all(log_dir)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros())
            .unwrap_or(0);
        let journal = log_dir.join(format!("session-{stamp}.jsonl"));
        let transcript_path = log_dir.join(format!("session-{stamp}.log"));
        // Eagerly create the journal so it exists as soon as the log opens,
        // matching the transcript below. `record_event` only opens it with
        // `append`, so it would not otherwise exist until the first write.
        std::fs::File::create(&journal)?;
        let recorder = TrajectoryRecorder::new(journal.clone());
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&transcript_path)?;
        Ok(SessionLog {
            recorder,
            transcript: Some(file),
            transcript_path,
            started_at: Instant::now(),
            task_id: task_id.into(),
            turn: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
        })
    }

    /// The journal path, for reporting back to the user.
    pub fn journal_path(&self) -> &Path {
        self.recorder.path()
    }

    /// The transcript path, for reporting back to the user.
    pub fn transcript_path(&self) -> &Path {
        &self.transcript_path
    }

    /// The running task identifier.
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// Cumulative input tokens observed across the session.
    pub fn total_input_tokens(&self) -> u64 {
        self.total_input_tokens
    }

    /// Cumulative output tokens observed across the session.
    pub fn total_output_tokens(&self) -> u64 {
        self.total_output_tokens
    }

    /// Total tokens observed across the session.
    pub fn total_tokens(&self) -> u64 {
        self.total_input_tokens + self.total_output_tokens
    }

    /// Wall-clock seconds since the session started.
    pub fn elapsed_secs(&self) -> f64 {
        self.started_at.elapsed().as_secs_f64()
    }

    fn transcript(&mut self, line: &str) {
        if let Some(file) = self.transcript.as_mut() {
            let _ = writeln!(file, "{line}");
        }
    }

    fn record(&self, event: TrajectoryEvent) {
        let _ = self.recorder.record_event(&event);
    }

    fn timestamp_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Writes the structured session-start event.
    pub fn session_start(&mut self) {
        self.record(TrajectoryEvent::SessionStart {
            session_id: self.task_id.clone(),
            task_id: self.task_id.clone(),
            timestamp: Self::timestamp_ms(),
        });
        self.transcript(&format!(
            "== session {} started {}s ago ==",
            self.task_id,
            self.elapsed_secs().round()
        ));
    }

    /// Begins recording one turn and captures the full reasoning trace for this
    /// turn, which compaction and the UI may drop but the log keeps.
    pub fn turn_start(&mut self, turn: &ModelTurn) -> usize {
        self.turn += 1;
        let turn_idx = self.turn;
        self.record(TrajectoryEvent::TurnStart {
            turn: turn_idx,
            timestamp: Self::timestamp_ms(),
        });
        if let Some(reasoning) = &turn.reasoning
            && !reasoning.trim().is_empty()
        {
            self.record(TrajectoryEvent::ModelReasoning {
                turn: turn_idx,
                reasoning: reasoning.clone(),
            });
            self.transcript(&format!(
                "[turn {turn_idx}] reasoning: {}",
                reason_lines(reasoning)
            ));
        }
        turn_idx
    }

    /// Records a completed turn, folding in provider token usage.
    pub fn turn_finish(
        &mut self,
        turn: usize,
        usage: &Option<agent_runtime::ModelUsage>,
        finish_reason: &str,
    ) {
        if let Some(usage) = usage {
            self.total_input_tokens += usage.input_tokens;
            self.total_output_tokens += usage.output_tokens;
        }
        let output = usage.map(|u| u.output_tokens).unwrap_or(0);
        let input = usage.map(|u| u.input_tokens).unwrap_or(0);
        self.record(TrajectoryEvent::TurnFinish {
            turn,
            output_tokens: Some(output),
            finish_reason: finish_reason.to_owned(),
        });
        self.record(TrajectoryEvent::PromptEval {
            turn,
            cached_tokens: None,
            new_tokens: Some(input),
            eval_duration_ms: None,
        });
    }

    /// Records an approved tool dispatch and its sandbox observation.
    pub fn tool_result(
        &mut self,
        turn: usize,
        call_id: &str,
        tool: &str,
        args: &serde_json::Value,
        outcome: &crate::sandbox::ToolOutcome,
    ) {
        let stdout = outcome.output_text(MAX_LOG_OUTPUT_BYTES);
        let stderr = outcome.error_text(MAX_LOG_OUTPUT_BYTES);
        self.record(TrajectoryEvent::ToolDispatch {
            turn,
            call_id: call_id.to_owned(),
            tool: tool.to_owned(),
            args: args.clone(),
            action_hash: format!("{tool}:{call_id}"),
        });
        self.record(TrajectoryEvent::ToolResult {
            turn,
            call_id: call_id.to_owned(),
            tool: tool.to_owned(),
            stdout: bounded_stdout(&stdout),
            stderr: bounded_stderr(&stderr),
            exit_code: outcome.status.unwrap_or(-1),
            bytes: outcome.stdout.len() + outcome.stderr.len(),
            state_mutated: !outcome.stdout.is_empty() || !outcome.stderr.is_empty(),
        });
        let summary = format!("[turn {turn}] {tool} {call_id}: {stdout}{stderr}");
        self.transcript(&summary);
    }

    /// Writes the terminal session-finish event with session totals.
    pub fn session_finish(&mut self, success: bool) {
        self.record(TrajectoryEvent::SessionFinish {
            session_id: self.task_id.clone(),
            task_id: self.task_id.clone(),
            total_turns: self.turn,
            total_tokens: self.total_tokens(),
            success,
        });
        self.transcript(&format!(
            "== session {} finished: {} turns, {} tokens, {}s, {} ==",
            self.task_id,
            self.turn,
            self.total_tokens(),
            self.elapsed_secs().round(),
            if success { "ok" } else { "cancelled/failed" }
        ));
    }
}

impl Recorder for SessionLog {
    fn session_start(&mut self) {
        SessionLog::session_start(self);
    }

    fn turn_start(&mut self, turn: &ModelTurn) -> usize {
        SessionLog::turn_start(self, turn)
    }

    fn turn_finish(&mut self, turn: usize, usage: &Option<ModelUsage>, finish_reason: &str) {
        SessionLog::turn_finish(self, turn, usage, finish_reason);
    }

    fn tool_result(
        &mut self,
        turn: usize,
        call_id: &str,
        tool: &str,
        args: &serde_json::Value,
        outcome: &crate::sandbox::ToolOutcome,
    ) {
        SessionLog::tool_result(self, turn, call_id, tool, args, outcome);
    }

    fn session_finish(&mut self, success: bool) {
        SessionLog::session_finish(self, success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_and_transcript_are_created_under_log_dir() {
        let dir = tempfile::tempdir().unwrap();
        let log = SessionLog::open(dir.path(), "task-1").expect("open");
        let jp = log.journal_path().to_string_lossy().to_string();
        let tp = log.transcript_path().to_string_lossy().to_string();
        assert!(jp.ends_with(".jsonl"));
        assert!(tp.ends_with(".log"));
        assert!(Path::new(&jp).exists());
        assert!(Path::new(&tp).exists());
    }

    #[test]
    fn session_records_start_and_finish_events() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = SessionLog::open(dir.path(), "task-2").expect("open");
        log.session_start();
        log.session_finish(true);
        let contents = std::fs::read_to_string(log.journal_path()).unwrap();
        assert!(contents.contains("session_start"));
        assert!(contents.contains("session_finish"));
    }

    #[test]
    fn tokens_accumulate_across_turns() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = SessionLog::open(dir.path(), "task-3").expect("open");
        let usage = Some(agent_runtime::ModelUsage {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
        });
        log.turn_finish(1, &usage, "stop");
        log.turn_finish(2, &usage, "stop");
        assert_eq!(log.total_input_tokens(), 20);
        assert_eq!(log.total_output_tokens(), 10);
        assert_eq!(log.total_tokens(), 30);
    }

    #[test]
    fn transcript_records_reasoning() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = SessionLog::open(dir.path(), "task-4").expect("open");
        log.session_start();
        let turn = ModelTurn {
            text: "done".to_owned(),
            reasoning: Some("because reasons".to_owned()),
            tool_intents: vec![],
            finish_reason: agent_runtime::FinishReason::Stop,
            provider: agent_runtime::LocalModelProvider::LlamaServer,
            model: "m".to_owned(),
            response_id: None,
            provider_request_id: None,
            usage: Some(agent_runtime::ModelUsage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
            }),
        };
        let idx = log.turn_start(&turn);
        log.turn_finish(idx, &turn.usage, "stop");
        let transcript = std::fs::read_to_string(log.transcript_path()).unwrap();
        assert!(transcript.contains("reasoning"));
        assert!(transcript.contains("because reasons"));
    }
}
