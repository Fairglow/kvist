//! Composite session recorder that writes both the NDJSON journal and the
//! structured transcript format that Blad reads.
//!
//! The NDJSON journal (.jsonl) is the operational record. The structured
//! transcript (session.json manifest + messages/*.md) enables source-derived
//! re-theming and faithful reproduction in any viewer.

use std::io;
use std::path::Path;

use sav::{ModelRequest, ModelTurn, ModelUsage, ToolIntent};

use crate::error::Result;
use crate::sandbox::ToolOutcome;
use crate::session::{Recorder, RunSummary};
use crate::session_log::SessionLog;
use crate::transcript_writer::TranscriptWriter;

/// A recorder that writes both the NDJSON journal and the structured transcript.
pub struct SessionTranscript {
    log: SessionLog,
    transcript: TranscriptWriter,
}

impl SessionTranscript {
    /// Opens a new session, creating both the journal files and the structured
    /// transcript directory.
    pub fn open(
        log_dir: &Path,
        task_id: impl Into<String>,
        model: Option<&str>,
    ) -> io::Result<Self> {
        let log = SessionLog::open(log_dir, task_id)?;
        // The transcript directory uses the same naming convention as the journal
        // files (session-{id}) so it can be found by matching the session id.
        let transcript_dir = format!("session-{}", log.session_id());
        let transcript = TranscriptWriter::new(&transcript_dir, log_dir, model)?;
        Ok(Self { log, transcript })
    }

    /// Attaches caller-supplied operational scope.
    pub fn with_metadata(mut self, metadata: crate::session_log::SessionMetadata) -> Self {
        self.log = self.log.with_metadata(metadata);
        self
    }

    /// Location of the version-one journal.
    pub fn journal_path(&self) -> &Path {
        self.log.journal_path()
    }

    /// Location of the bounded, potentially sensitive transcript.
    pub fn transcript_path(&self) -> &Path {
        self.log.transcript_path()
    }

    /// Caller-supplied descriptive label.
    pub fn task_id(&self) -> &str {
        self.log.task_id()
    }

    /// Folds the first few words of a prompt into the session id.
    pub fn annotate_prompt(&mut self, prompt: &str) -> io::Result<()> {
        self.log.annotate_prompt(prompt)
    }

    /// Total input tokens observed.
    pub fn total_input_tokens(&self) -> u64 {
        self.log.total_input_tokens()
    }

    /// Total output tokens observed.
    pub fn total_output_tokens(&self) -> u64 {
        self.log.total_output_tokens()
    }

    /// Total tokens observed.
    pub fn total_tokens(&self) -> u64 {
        self.log.total_tokens()
    }

    /// Elapsed wall-clock time since the session started.
    pub fn elapsed_secs(&self) -> f64 {
        self.log.elapsed_secs()
    }
}

impl Recorder for SessionTranscript {
    fn notice(&mut self, message: &str) -> Result<()> {
        self.log.notice(message)?;
        match self.transcript.record_notice(message) {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::error!(%e, "record transcript notice failed");
                Err(crate::error::io_error("record transcript notice", None, e))
            }
        }
    }

    fn on_prompt(&mut self, prompt: &str) -> Result<()> {
        self.log.on_prompt(prompt)?;
        self.transcript
            .record_user(prompt)
            .map_err(|e| crate::error::io_error("record transcript user message", None, e))?;
        Ok(())
    }

    fn session_start(&mut self) -> Result<()> {
        self.log.session_start()
    }

    fn request(&mut self, request: &ModelRequest, attempt: u32) -> Result<()> {
        self.log.request(request, attempt)
    }

    fn turn_start(&mut self, turn: &ModelTurn) -> Result<usize> {
        let idx = self.log.turn_start(turn)?;
        if let Some(reasoning) = turn.reasoning.as_deref()
            && !reasoning.is_empty() {
                self.transcript
                    .record_reasoning(reasoning)
                    .map_err(|e| crate::error::io_error("record transcript reasoning", None, e))?;
            }
        Ok(idx)
    }

    fn turn_finish(
        &mut self,
        turn: usize,
        usage: &Option<ModelUsage>,
        finish_reason: &str,
    ) -> Result<()> {
        self.log.turn_finish(turn, usage, finish_reason)
    }

    fn tool_dispatch(&mut self, turn: usize, intent: &ToolIntent) -> Result<()> {
        self.log.tool_dispatch(turn, intent)?;
        let args = serde_json::to_string(&intent.arguments).unwrap_or_default();
        self.transcript
            .record_tool_dispatch(&intent.name, &args)
            .map_err(|e| crate::error::io_error("record transcript tool dispatch", None, e))?;
        Ok(())
    }

    fn tool_result(
        &mut self,
        turn: usize,
        call_id: &str,
        tool: &str,
        args: &serde_json::Value,
        outcome: &ToolOutcome,
    ) -> Result<()> {
        self.log.tool_result(turn, call_id, tool, args, outcome)?;
        let output = String::from_utf8_lossy(&outcome.stdout);
        if !output.trim().is_empty() {
            self.transcript
                .record_tool_result(tool, &output)
                .map_err(|e| crate::error::io_error("record transcript tool result", None, e))?;
        }
        Ok(())
    }

    fn session_finish(&mut self, summary: &RunSummary) -> Result<()> {
        self.log.session_finish(summary)?;
        if let Some(answer) = summary.answer.as_deref() {
            self.transcript
                .record_assistant(answer, true)
                .map_err(|e| crate::error::io_error("record transcript final answer", None, e))?;
        }
        self.transcript
            .complete()
            .map_err(|e| crate::error::io_error("complete transcript", None, e))?;
        Ok(())
    }
}
