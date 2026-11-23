use std::{fs, io::Write, path::Path};

use sav::{TrajectoryEvent as Event, TrajectoryRecorder};

use super::{
    AgentExecutionRequest, AgentRunResult, RunTurn, TokenBudget, TurnFailure, decision_markdown,
    redact, redact_bounded,
};
use crate::{KvistError, Result};

/// Retains the result and bounded text, but writes per-turn detail to disk.
pub(super) struct RunHistory<'a> {
    recorder: TrajectoryRecorder,
    session_id: String,
    task_id: String,
    redactions: &'a [String],
    max_bytes: usize,
    prose: String,
    broker: String,
    decisions: String,
    answer: String,
    usable_answer: bool,
    dropped: bool,
    unapplied: bool,
    surfaced_decision: bool,
    pub(super) turns: usize,
    reported: usize,
    pub(super) reads: usize,
    pub(super) edits: usize,
    pub(super) timed_out: bool,
    output_limit_exceeded: bool,
    pub(super) cancelled: bool,
}

impl<'a> RunHistory<'a> {
    pub(super) fn start(
        request: &AgentExecutionRequest<'_>,
        session_id: &str,
        redactions: &'a [String],
        max_bytes: usize,
    ) -> Result<Self> {
        let recorder = TrajectoryRecorder::new(
            request
                .target_dir
                .join(".kvist/runs")
                .join(format!("{session_id}.trajectory.jsonl")),
        );
        let session_id = redact(session_id, redactions);
        let task_id = redact(request.task_id, redactions);
        recorder.record_event(&Event::SessionStart {
            session_id: session_id.clone(),
            task_id: task_id.clone(),
            timestamp: timestamp(),
        })?;
        Ok(Self {
            recorder,
            session_id,
            task_id,
            redactions,
            max_bytes,
            prose: String::new(),
            broker: String::new(),
            decisions: String::new(),
            answer: String::new(),
            usable_answer: false,
            dropped: false,
            unapplied: false,
            surfaced_decision: false,
            turns: 0,
            reported: 0,
            reads: 0,
            edits: 0,
            timed_out: false,
            output_limit_exceeded: false,
            cancelled: false,
        })
    }

    pub(super) fn record(&mut self, current: &RunTurn) -> Result<()> {
        let turn = &current.turn;
        self.turns += 1;
        self.reported += usize::from(turn.usage.is_some());
        self.dropped |= !current.drops.is_empty();
        self.surfaced_decision |= !current.surfaced_decisions.is_empty();
        self.usable_answer = !turn.text.trim().is_empty() || !turn.tool_intents.is_empty();
        self.answer = redact_bounded(&turn.text, self.redactions, self.max_bytes);
        push(
            &mut self.prose,
            self.max_bytes,
            &format!("\nturn {}:\n", self.turns),
            &[],
        );
        if let Some(reasoning) = turn.reasoning.as_deref().filter(|s| !s.trim().is_empty()) {
            push(&mut self.prose, self.max_bytes, "reasoning:\n", &[]);
            push(&mut self.prose, self.max_bytes, reasoning, self.redactions);
            push(&mut self.prose, self.max_bytes, "\n\n", &[]);
        }
        push(&mut self.prose, self.max_bytes, &turn.text, self.redactions);
        push(&mut self.prose, self.max_bytes, "\n", &[]);

        self.recorder.record_event(&Event::TurnStart {
            turn: self.turns,
            timestamp: timestamp(),
        })?;
        self.recorder.record_event(&Event::PromptEval {
            turn: self.turns,
            cached_tokens: None,
            new_tokens: turn.usage.map(|usage| usage.input_tokens),
            eval_duration_ms: None,
        })?;
        for read in &current.reads {
            self.reads += 1;
            push(
                &mut self.broker,
                self.max_bytes,
                &format!(
                    "read {} -> {} ({} bytes)\n",
                    read.tool,
                    read.path,
                    read.content.len()
                ),
                self.redactions,
            );
            self.tool(
                &read.call_id,
                &read.tool,
                serde_json::json!({ "path": redact(&read.path, self.redactions) }),
                &read.content,
                "",
                read.outcome.exit_code(),
                read.content.len(),
                false,
            )?;
        }
        for effect in &current.effects {
            self.edits += usize::from(effect.applied);
            self.unapplied |= !effect.applied;
            self.timed_out |= effect.timed_out;
            self.output_limit_exceeded |= effect.output_limit_exceeded;
            self.cancelled |= effect.cancelled;
            push(
                &mut self.broker,
                self.max_bytes,
                &format!(
                    "{} {} -> {}\n{}\n",
                    if effect.applied { "applied" } else { "FAILED" },
                    effect.tool,
                    effect.destination,
                    effect.detail
                ),
                self.redactions,
            );
            self.tool(
                &effect.call_id,
                &effect.tool,
                serde_json::json!({ "destination": redact(&effect.destination, self.redactions) }),
                if effect.applied { "applied" } else { "" },
                &effect.detail,
                i32::from(!effect.applied),
                effect.detail.len(),
                // This legacy boolean cannot encode uncertainty.
                effect.applied || effect.uncertain,
            )?;
        }
        for drop in &current.drops {
            push(
                &mut self.broker,
                self.max_bytes,
                &format!(
                    "dropped call {} (tool `{}`): {}\n",
                    drop.call_id, drop.tool, drop.reason
                ),
                self.redactions,
            );
        }
        for decision in &current.surfaced_decisions {
            let summary = decision_markdown(decision);
            push(&mut self.answer, self.max_bytes, "\n\n", &[]);
            push(&mut self.answer, self.max_bytes, &summary, self.redactions);
            push(
                &mut self.decisions,
                self.max_bytes,
                &format!(
                    "decision {} ({}): {}\n",
                    decision.call_id, decision.summary, decision.why
                ),
                self.redactions,
            );
            if let Some(patch) = &decision.patch {
                push(&mut self.decisions, self.max_bytes, "  patch:\n", &[]);
                push(&mut self.decisions, self.max_bytes, patch, self.redactions);
                push(&mut self.decisions, self.max_bytes, "\n", &[]);
            }
        }
        for dependency in &current.accepted_requests {
            push(
                &mut self.decisions,
                self.max_bytes,
                &format!(
                    "dependency {} -> {} ({})\n",
                    dependency.name,
                    dependency.origin,
                    dependency.reason.as_deref().unwrap_or("accepted")
                ),
                self.redactions,
            );
        }
        let finish_reason = serde_json::to_value(&turn.finish_reason)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "stop".to_owned());
        self.recorder.record_event(&Event::TurnFinish {
            turn: self.turns,
            output_tokens: turn.usage.map(|usage| usage.output_tokens),
            finish_reason: redact(&finish_reason, self.redactions),
        })?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn tool(
        &self,
        call_id: &str,
        tool: &str,
        args: serde_json::Value,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
        bytes: usize,
        state_mutated: bool,
    ) -> Result<()> {
        let tool = redact(tool, self.redactions);
        let call_id = redact(call_id, self.redactions);
        self.recorder.record_event(&Event::ToolDispatch {
            turn: self.turns,
            call_id: call_id.clone(),
            tool: tool.clone(),
            action_hash: sav::compute_action_hash(&tool, &args),
            args,
        })?;
        self.recorder.record_event(&Event::ToolResult {
            turn: self.turns,
            call_id,
            tool,
            stdout: redact_bounded(stdout, self.redactions, self.max_bytes),
            stderr: redact_bounded(stderr, self.redactions, self.max_bytes),
            exit_code,
            bytes,
            state_mutated,
        })?;
        Ok(())
    }

    pub(super) fn finish(
        self,
        log_path: &Path,
        log_file: &mut fs::File,
        failure: Option<&TurnFailure>,
        tokens: &TokenBudget,
    ) -> Result<AgentRunResult> {
        let success = self.usable_answer
            && failure.is_none()
            && !self.dropped
            && !self.unapplied
            && !self.surfaced_decision;
        let usage = tokens.known_usage();
        let tokens_input = usage.map(|usage| usage.input_tokens.min(usize::MAX as u64) as usize);
        let tokens_output = usage.map(|usage| usage.output_tokens.min(usize::MAX as u64) as usize);
        let accounting = tokens.accounting();
        let uncertain_attempts = tokens.uncertain_attempts;
        let mut log = redact_bounded(
            &format!(
                "token accounting: known input={tokens_input:?}, known output={tokens_output:?}; \
             reported turns={}, unavailable turns={}, conservatively charged/unknown attempts={uncertain_attempts}\n\
             cumulative provenance={}; returned attempts={}, known attempts={}; complete cumulative usage={:?}\n\n",
                self.reported,
                self.turns - self.reported,
                if accounting.provenance_complete {
                    "complete"
                } else {
                    "unavailable"
                },
                accounting.returned_attempts,
                accounting.known_attempts,
                accounting.cumulative_usage,
            ),
            self.redactions,
            self.max_bytes,
        );
        let stderr = failure.map_or_else(String::new, |failure| {
            redact_bounded(&failure.reason, self.redactions, self.max_bytes)
        });
        if failure.is_some() {
            push(&mut log, self.max_bytes, "--- failure ---\n", &[]);
            push(&mut log, self.max_bytes, &stderr, &[]);
            push(&mut log, self.max_bytes, "\n\n", &[]);
        }
        push(&mut log, self.max_bytes, &self.prose, &[]);
        if !self.broker.is_empty() {
            push(
                &mut log,
                self.max_bytes,
                "\n--- authoring broker ---\n",
                &[],
            );
            push(&mut log, self.max_bytes, &self.broker, &[]);
        }
        if !self.decisions.is_empty() {
            push(
                &mut log,
                self.max_bytes,
                "\n--- surfaced decisions and dependency requests ---\n",
                &[],
            );
            push(&mut log, self.max_bytes, &self.decisions, &[]);
        }
        log_file
            .write_all(redact_bounded(&log, self.redactions, self.max_bytes).as_bytes())
            .map_err(|source| KvistError::Io {
                operation: "write agent log",
                path: log_path.to_owned(),
                source,
            })?;
        log_file.sync_all().map_err(|source| KvistError::Io {
            operation: "sync agent log",
            path: log_path.to_owned(),
            source,
        })?;
        self.recorder.record_event(&Event::SessionFinish {
            session_id: self.session_id,
            task_id: self.task_id,
            total_turns: self.turns,
            total_tokens: usage.map_or(0, |usage| {
                usage.input_tokens.saturating_add(usage.output_tokens)
            }),
            success,
        })?;
        Ok(AgentRunResult {
            success,
            tokens_input,
            tokens_output,
            log_path: log_path.to_owned(),
            stdout: self.answer,
            stderr,
            timed_out: self.timed_out || failure.is_some_and(|failure| failure.timed_out),
            output_limit_exceeded: self.output_limit_exceeded
                || failure.is_some_and(|failure| failure.output_limit_exceeded),
            surfaced_decision: self.surfaced_decision,
        })
    }
}

fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn push(out: &mut String, limit: usize, text: &str, redactions: &[String]) {
    let remaining = limit.saturating_sub(out.len());
    if remaining != 0 {
        out.push_str(&redact_bounded(text, redactions, remaining));
    }
}
