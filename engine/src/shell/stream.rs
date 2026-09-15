use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::cli;

use super::style::{self, Theme};

/// Delay before the spinner becomes visible, so fast commands never flicker.
const SPINNER_DELAY: Duration = Duration::from_millis(150);

/// Formats an elapsed-time label: seconds under a minute, minutes and
/// seconds under an hour, hours and minutes beyond that.
pub(crate) fn format_elapsed(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// A non-blocking background terminal spinner for transient command execution.
///
/// The spinner starts lazily (after a short delay) so fast commands never
/// flicker, and it displays elapsed time so long-running work is visibly
/// progressing rather than hung. It draws only when stderr is an interactive
/// terminal; captured output is never polluted with cursor control.
pub struct ProgressSpinner {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProgressSpinner {
    /// Starts an asynchronous spinner on stderr displaying `message`.
    pub fn start(message: String) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let tty = io::stderr().is_terminal();
        let running_clone = running.clone();

        let handle = thread::spawn(move || {
            if !tty {
                // Nothing to draw; just wait for shutdown.
                while running_clone.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(100));
                }
                return;
            }
            let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let started = Instant::now();
            let mut idx = 0;
            while running_clone.load(Ordering::Relaxed) {
                let elapsed = started.elapsed();
                if elapsed < SPINNER_DELAY {
                    // Deferred start: fast commands never see a spinner.
                    thread::sleep(Duration::from_millis(16));
                    continue;
                }
                let char = spinner_chars[idx % spinner_chars.len()];
                let _ = io::stderr().write_all(
                    format!(
                        "\r  {char} {message} ({})",
                        format_elapsed(elapsed.as_secs())
                    )
                    .as_bytes(),
                );
                let _ = io::stderr().flush();
                idx += 1;
                thread::sleep(Duration::from_millis(100));
            }
            // Erase the transient spinner line completely using ANSI codes
            let _ = io::stderr().write_all(b"\r\x1b[2K");
            let _ = io::stderr().flush();
        });

        Self {
            running,
            handle: Some(handle),
        }
    }

    /// Stops the spinner and erases the transient line from the terminal.
    pub fn stop(mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ProgressSpinner {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Summarized feedback extracted from the most recent agent execution run.
#[derive(Debug, Clone, Default)]
pub struct AgentFeedback {
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub turns: usize,
    pub tool_calls: usize,
    pub tokens_input: Option<usize>,
    pub tokens_output: Option<usize>,
    pub reasoning: Option<String>,
    pub trajectory_path: Option<PathBuf>,
    pub log_path: Option<PathBuf>,
}

/// Re-exports the feedback target so shell callers keep one import site.
pub use super::runs::FeedbackTarget;

/// Manages transient output streams and log link replacement for execution.
pub struct StreamManager {
    project_root: PathBuf,
    theme: Theme,
}

impl StreamManager {
    /// Creates a new stream manager for the given project root.
    pub fn new(project_root: &Path, theme: Theme) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            theme,
        }
    }

    /// The probed terminal width, when known, for fitting stage boxes.
    fn width(&self) -> Option<usize> {
        style::terminal_size().map(|(width, _)| width)
    }

    /// The maximum visible columns for one stage row at the current width.
    fn row_limit(&self) -> usize {
        self.width()
            .map(|width| width.saturating_sub(4))
            .unwrap_or(160)
    }

    /// Starts a non-blocking progress spinner for a long-running command.
    pub fn start_spinner(&self, command_name: &str) -> ProgressSpinner {
        ProgressSpinner::start(format!("Running {command_name}"))
    }

    /// Prints the Prompt stage: a closed box holding the command line.
    pub fn print_prompt_stage(&self, prompt_line: &str) {
        let titled = style::titled_box(
            self.theme,
            "Prompt",
            &[crate::shell::truncate(prompt_line, self.row_limit())],
            self.width(),
        );
        println!(
            "{}\n{}\n{}",
            titled.top,
            titled.rows.join("\n"),
            titled.bottom
        );
    }

    /// Prints the Agent Working stage header (the box stays open until the
    /// result stage closes it at the same width).
    pub fn print_working_stage(&self) {
        let titled = style::titled_box(self.theme, "Agent Working", &[], self.width());
        println!("{}", titled.top);
    }

    /// Finds the agent execution feedback to show after a run, correlated with
    /// the requested target (a specific task or the most recent run).
    pub fn find_latest_agent_feedback(&self, target: &FeedbackTarget<'_>) -> Option<AgentFeedback> {
        let runs = super::runs::scan_recent_runs(&self.project_root, usize::MAX);
        let run = super::runs::select_recent_run(&runs, target)?;
        let traj_path = run.trajectory_path.clone()?;
        let report = agent_runtime::replay_trajectory(&traj_path, None).ok()?;
        let mut reasoning = None;
        // Run-record totals are authoritative; only when the record leaves a
        // direction open do we accumulate the per-turn trajectory metrics.
        let record_in = run.tokens_input.map(|t| t as usize);
        let record_out = run.tokens_output.map(|t| t as usize);
        let mut tokens_in = record_in;
        let mut tokens_out = record_out;

        for event in &report.events {
            match event {
                agent_runtime::TrajectoryEvent::ModelReasoning { reasoning: r, .. }
                    if reasoning.is_none() =>
                {
                    reasoning = Some(r.clone());
                }
                agent_runtime::TrajectoryEvent::PromptEval {
                    new_tokens: Some(t),
                    ..
                } if record_in.is_none() => {
                    tokens_in = match tokens_in {
                        Some(acc) => Some(acc.saturating_add(*t as usize)),
                        None => Some(*t as usize),
                    };
                }
                agent_runtime::TrajectoryEvent::TurnFinish {
                    output_tokens: Some(t),
                    ..
                } if record_out.is_none() => {
                    tokens_out = match tokens_out {
                        Some(acc) => Some(acc.saturating_add(*t as usize)),
                        None => Some(*t as usize),
                    };
                }
                agent_runtime::TrajectoryEvent::SessionFinish { total_tokens, .. }
                    if tokens_in.is_none() && tokens_out.is_none() =>
                {
                    tokens_out = Some(*total_tokens as usize);
                }
                _ => {}
            }
        }

        // Logs live next to the run records, in the same component's .kvist dir,
        // named `<task_id>_<timestamp>.log`.
        let mut log_path = None;
        if let Some(runs_dir) = traj_path.parent()
            && let Some(kvist_dir) = runs_dir.parent()
            && let Some(task_id) = &run.task_id
            && let Ok(entries) = fs::read_dir(kvist_dir.join("logs"))
        {
            let prefix = format!("{task_id}_");
            let mut newest_log = None;
            for entry in entries.flatten() {
                let path = entry.path();
                let fname = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if fname.starts_with(&prefix)
                    && fname.ends_with(".log")
                    && let Ok(meta) = path.metadata()
                    && let Ok(modified) = meta.modified()
                    && newest_log.as_ref().is_none_or(|(_, best)| modified > *best)
                {
                    newest_log = Some((path, modified));
                }
            }
            log_path = newest_log.map(|(p, _)| p);
        }

        Some(AgentFeedback {
            session_id: report.session_id,
            task_id: report.task_id.or(run.task_id.clone()),
            turns: report.replayed_turns,
            tool_calls: report.tool_calls_count,
            tokens_input: tokens_in,
            tokens_output: tokens_out,
            reasoning,
            trajectory_path: Some(traj_path),
            log_path,
        })
    }

    /// Builds the content rows of the Agent Result stage (pure, so the
    /// output can be asserted without a terminal).
    pub fn result_stage_rows(
        &self,
        result: &std::result::Result<cli::CommandOutput, crate::KvistError>,
        target: &FeedbackTarget<'_>,
    ) -> Vec<String> {
        let limit = self.row_limit();
        let mut rows: Vec<String> = Vec::new();
        match result {
            Ok(output) => {
                let text = output.to_string();
                let trimmed = text.trim();
                for line in trimmed.lines() {
                    rows.push(crate::shell::truncate(line, limit));
                }
                if let Some(feedback) = self.find_latest_agent_feedback(target) {
                    rows.push(String::new());
                    rows.push("Agent Feedback:".to_owned());
                    if let (Some(in_tok), Some(out_tok)) =
                        (feedback.tokens_input, feedback.tokens_output)
                    {
                        let total = in_tok + out_tok;
                        rows.push(format!(
                            "• Tokens: {in_tok} input · {out_tok} output ({total} total)"
                        ));
                    }
                    if feedback.turns > 0 || feedback.tool_calls > 0 {
                        rows.push(format!(
                            "• Activity: {} turn(s), {} tool call(s)",
                            feedback.turns, feedback.tool_calls
                        ));
                    }
                    if let Some(reasoning) = &feedback.reasoning {
                        // `truncate` is character-based, so multi-byte text
                        // never panics on a byte boundary.
                        let snippet = crate::shell::truncate(reasoning, 80);
                        rows.push(format!("• Reasoning: \"{snippet}\""));
                    }
                    if let Some(traj) = &feedback.trajectory_path
                        && let Ok(rel) = traj.strip_prefix(&self.project_root)
                    {
                        rows.push(format!("• Trajectory: ./{rel}", rel = rel.display()));
                    }
                    if let Some(log) = &feedback.log_path
                        && let Ok(rel) = log.strip_prefix(&self.project_root)
                    {
                        rows.push(format!("• Full log: ./{rel}", rel = rel.display()));
                    }
                }
            }
            Err(error) => {
                rows.push(format!("✘ Execution failed: {error}"));
            }
        }
        rows
    }

    /// Closes the Agent Working box and prints the Agent Result and Feedback
    /// stage as a closed box that fits the terminal width.
    pub fn print_result_stage(
        &self,
        result: &std::result::Result<cli::CommandOutput, crate::KvistError>,
        target: &FeedbackTarget<'_>,
    ) {
        let close = style::titled_box(self.theme, "Agent Working", &[], self.width()).bottom;
        let rows = self.result_stage_rows(result, target);
        let titled = style::titled_box(self.theme, "Agent Result", &rows, self.width());
        println!("{}\n{}\n{}", close, titled.top, titled.rows.join("\n"));
        println!("{}", titled.bottom);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::runs::write_trajectory;
    use tempfile::tempdir;

    #[test]
    fn spinner_starts_and_stops_cleanly() {
        let spinner = ProgressSpinner::start("Testing".into());
        thread::sleep(Duration::from_millis(250));
        spinner.stop();
    }

    #[test]
    fn format_elapsed_renders_units() {
        assert_eq!(format_elapsed(0), "0s");
        assert_eq!(format_elapsed(59), "59s");
        assert_eq!(format_elapsed(60), "1m 0s");
        assert_eq!(format_elapsed(3725), "1h 2m");
    }

    #[test]
    fn stage_printing_executes_without_panicking() {
        let dir = tempdir().unwrap();
        let manager = StreamManager::new(dir.path(), Theme::plain());
        manager.print_prompt_stage("task run . implement-code");
        manager.print_working_stage();
        manager.print_result_stage(
            &Ok(cli::CommandOutput::message("success")),
            &FeedbackTarget::Latest,
        );
        manager.print_result_stage(
            &Err(crate::KvistError::AgentRuntime(
                agent_runtime::Error::Cancelled,
            )),
            &FeedbackTarget::None,
        );
    }

    #[test]
    fn result_stage_rows_keep_the_full_error_detail() {
        let dir = tempdir().unwrap();
        let manager = StreamManager::new(dir.path(), Theme::plain());
        let rows = manager.result_stage_rows(
            &Err(crate::KvistError::AgentRuntime(
                agent_runtime::Error::Cancelled,
            )),
            &FeedbackTarget::None,
        );
        assert_eq!(rows.len(), 1);
        assert!(rows[0].starts_with("✘ Execution failed: "));
        assert!(rows[0].contains("cancelled"));

        let rows = manager.result_stage_rows(
            &Ok(cli::CommandOutput::message("line one\nline two")),
            &FeedbackTarget::None,
        );
        assert_eq!(rows, vec!["line one".to_owned(), "line two".to_owned()]);
    }

    #[test]
    fn result_stage_rows_truncates_multibyte_reasoning_on_a_char_boundary() {
        let dir = tempdir().unwrap();
        let runs_dir = dir.path().join(".kvist").join("runs");
        fs::create_dir_all(&runs_dir).unwrap();
        // A 100-character multi-byte reasoning snippet: truncation must stay
        // on a character boundary and mark the cut.
        let reasoning = "é".repeat(100);
        let journal = format!(
            r#"{{"event":"session_start","session_id":"s1","task_id":"task-mb","timestamp":1}}
{{"event":"model_reasoning","turn":1,"reasoning":"{reasoning}"}}
{{"event":"session_finish","session_id":"s1","task_id":"task-mb","total_turns":1,"total_tokens":10,"success":true}}
"#
        );
        fs::write(
            runs_dir.join("task-mb_2026-09-11T22-00-00Z.trajectory.jsonl"),
            journal,
        )
        .unwrap();

        let manager = StreamManager::new(dir.path(), Theme::plain());
        let rows = manager.result_stage_rows(
            &Ok(cli::CommandOutput::message("done")),
            &FeedbackTarget::Latest,
        );
        let reasoning_row = rows
            .iter()
            .find(|row| row.starts_with("• Reasoning:"))
            .expect("reasoning row present");
        let snippet = reasoning_row
            .trim_start_matches("• Reasoning: \"")
            .trim_end_matches("\"");
        assert!(snippet.ends_with("..."));
        // 77 visible characters plus the ellipsis, all valid UTF-8.
        assert_eq!(snippet.chars().count(), 80);
        assert!(snippet.chars().all(|c| c == 'é' || c == '.'));
    }

    #[test]
    fn result_stage_rows_include_the_feedback_block_with_links() {
        let dir = tempdir().unwrap();
        let kvist_dir = dir.path().join(".kvist");
        let runs_dir = kvist_dir.join("runs");
        let logs_dir = kvist_dir.join("logs");
        fs::create_dir_all(&runs_dir).unwrap();
        fs::create_dir_all(&logs_dir).unwrap();
        write_trajectory(&runs_dir, "task-fb", "2026-09-11T22-00-00Z");
        fs::write(
            runs_dir.join("task-fb_2026-09-11T22-00-00Z.json"),
            r#"{"status":"success","tokens_input":12,"tokens_output":8}"#,
        )
        .unwrap();
        fs::write(logs_dir.join("task-fb_2026-09-11T22-00-00Z.log"), "out").unwrap();

        let manager = StreamManager::new(dir.path(), Theme::plain());
        let rows = manager.result_stage_rows(
            &Ok(cli::CommandOutput::message("done")),
            &FeedbackTarget::Task("task-fb"),
        );
        assert!(rows.contains(&"Agent Feedback:".to_owned()));
        assert!(rows.contains(&"• Tokens: 12 input · 8 output (20 total)".to_owned()));
        assert!(rows.contains(&"• Activity: 1 turn(s), 1 tool call(s)".to_owned()));
        assert!(
            rows.iter()
                .any(|row| row.starts_with("• Trajectory: ./.kvist/runs/"))
        );
        assert!(
            rows.iter()
                .any(|row| row.starts_with("• Full log: ./.kvist/logs/"))
        );
    }

    #[test]
    fn find_latest_agent_feedback_extracts_metrics() {
        let dir = tempdir().unwrap();
        let runs_dir = dir.path().join(".kvist").join("runs");
        fs::create_dir_all(&runs_dir).unwrap();
        let traj_path = write_trajectory(&runs_dir, "task-1", "2026-09-11T22-00-00Z");

        let manager = StreamManager::new(dir.path(), Theme::plain());
        let feedback = manager
            .find_latest_agent_feedback(&FeedbackTarget::Latest)
            .expect("feedback present");
        assert_eq!(feedback.task_id.as_deref(), Some("task-1"));
        assert_eq!(feedback.turns, 1);
        assert_eq!(feedback.tool_calls, 1);
        assert_eq!(feedback.tokens_input, Some(50));
        assert_eq!(feedback.tokens_output, Some(25));
        assert_eq!(
            feedback.reasoning.as_deref(),
            Some("Inspecting test suite.")
        );
        assert_eq!(feedback.trajectory_path, Some(traj_path));
    }

    #[test]
    fn feedback_prefers_the_executed_task_over_newer_other_runs() {
        let dir = tempdir().unwrap();
        let runs_dir = dir.path().join(".kvist").join("runs");
        fs::create_dir_all(&runs_dir).unwrap();

        let older = write_trajectory(&runs_dir, "task-a", "2026-09-11T20-00-00Z");
        write_trajectory(&runs_dir, "task-b", "2026-09-11T22-00-00Z");

        let manager = StreamManager::new(dir.path(), Theme::plain());
        let feedback = manager
            .find_latest_agent_feedback(&FeedbackTarget::Task("task-a"))
            .expect("feedback for the requested task");
        assert_eq!(feedback.task_id.as_deref(), Some("task-a"));
        assert_eq!(feedback.trajectory_path, Some(older));
    }

    #[test]
    fn feedback_accumulates_trajectory_tokens_without_a_run_record() {
        let dir = tempdir().unwrap();
        let runs_dir = dir.path().join(".kvist").join("runs");
        fs::create_dir_all(&runs_dir).unwrap();
        let traj_path = runs_dir.join("task-x_2026-09-11T22-00-00Z.trajectory.jsonl");
        // Two prompt evaluations and two turn finishes; without a run record
        // the totals are the per-turn sums (50 + 30 in, 25 + 15 out).
        let journal = r#"{"event":"session_start","session_id":"s1","task_id":"task-x","timestamp":1}
{"event":"prompt_eval","turn":1,"new_tokens":50}
{"event":"turn_finish","turn":1,"output_tokens":25,"finish_reason":"stop"}
{"event":"prompt_eval","turn":2,"new_tokens":30}
{"event":"turn_finish","turn":2,"output_tokens":15,"finish_reason":"stop"}
{"event":"session_finish","session_id":"s1","task_id":"task-x","total_turns":2,"total_tokens":100,"success":true}
"#;
        fs::write(&traj_path, journal).unwrap();

        let manager = StreamManager::new(dir.path(), Theme::plain());
        let feedback = manager
            .find_latest_agent_feedback(&FeedbackTarget::Latest)
            .expect("feedback present");
        assert_eq!(feedback.tokens_input, Some(80));
        assert_eq!(feedback.tokens_output, Some(40));
    }

    #[test]
    fn feedback_prefers_run_record_totals_over_trajectory_sums() {
        let dir = tempdir().unwrap();
        let runs_dir = dir.path().join(".kvist").join("runs");
        fs::create_dir_all(&runs_dir).unwrap();
        write_trajectory(&runs_dir, "task-y", "2026-09-11T22-00-00Z");
        fs::write(
            runs_dir.join("task-y_2026-09-11T22-00-00Z.json"),
            r#"{"status":"completed","tokens_input":900,"tokens_output":400}"#,
        )
        .unwrap();

        let manager = StreamManager::new(dir.path(), Theme::plain());
        let feedback = manager
            .find_latest_agent_feedback(&FeedbackTarget::Latest)
            .expect("feedback present");
        assert_eq!(feedback.tokens_input, Some(900));
        assert_eq!(feedback.tokens_output, Some(400));
    }

    #[test]
    fn feedback_finds_the_component_log_next_to_the_runs_dir() {
        let dir = tempdir().unwrap();
        let kvist_dir = dir.path().join(".kvist");
        let runs_dir = kvist_dir.join("runs");
        let logs_dir = kvist_dir.join("logs");
        fs::create_dir_all(&runs_dir).unwrap();
        fs::create_dir_all(&logs_dir).unwrap();
        write_trajectory(&runs_dir, "task-z", "2026-09-11T22-00-00Z");
        let log = logs_dir.join("task-z_2026-09-11T22-00-00Z.log");
        fs::write(&log, "agent output").unwrap();
        // A different task's log must not be attributed to this run.
        fs::write(logs_dir.join("task-other_2026-09-11T23-00-00Z.log"), "x").unwrap();

        let manager = StreamManager::new(dir.path(), Theme::plain());
        let feedback = manager
            .find_latest_agent_feedback(&FeedbackTarget::Latest)
            .expect("feedback present");
        assert_eq!(feedback.log_path, Some(log));
    }
}
