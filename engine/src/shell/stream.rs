use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::{KvistError, Result, cli};

/// A non-blocking background terminal spinner for transient command execution.
pub struct ProgressSpinner {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProgressSpinner {
    /// Starts an asynchronous spinner on stderr displaying `message`.
    pub fn start(message: String) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let handle = thread::spawn(move || {
            let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let mut idx = 0;
            while running_clone.load(Ordering::Relaxed) {
                let char = spinner_chars[idx % spinner_chars.len()];
                let _ = io::stderr().write_all(format!("\r  {char} {message}...").as_bytes());
                let _ = io::stderr().flush();
                idx += 1;
                thread::sleep(Duration::from_millis(80));
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

/// Manages transient output streams and log link replacement for execution.
pub struct StreamManager {
    project_root: PathBuf,
    current_log: Mutex<Option<PathBuf>>,
}

impl StreamManager {
    /// Creates a new stream manager for the given project root.
    pub fn new(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            current_log: Mutex::new(None),
        }
    }

    /// Starts a non-blocking progress spinner for a long-running command.
    pub fn start_spinner(&self, command_name: &str) -> ProgressSpinner {
        ProgressSpinner::start(format!("Running {command_name}"))
    }

    /// Prepares a timestamped log path in `.kvist/logs/`.
    pub fn prepare_log_path(&self, program: &str) -> Result<PathBuf> {
        let log_dir = self.project_root.join(".kvist/logs");
        fs::create_dir_all(&log_dir).map_err(|e| KvistError::Io {
            operation: "create logs directory",
            path: log_dir.clone(),
            source: e,
        })?;
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
        let log_name = format!("{}_{}.log", program.replace('-', "_"), timestamp);
        let log_path = log_dir.join(log_name);
        if let Ok(mut guard) = self.current_log.lock() {
            *guard = Some(log_path.clone());
        }
        Ok(log_path)
    }

    /// Prints the styled Prompt stage header.
    pub fn print_prompt_stage(&self, prompt_line: &str) {
        println!("╭── Prompt ────────────────────────────────────────────────────────");
        println!("│  {prompt_line}");
        println!("╰──────────────────────────────────────────────────────────────────");
    }

    /// Prints the styled Agent Working stage header.
    pub fn print_working_stage(&self) {
        println!("╭── Agent Working ─────────────────────────────────────────────────");
    }

    /// Scans `.kvist/runs/` across the project for the latest session trajectory or run record.
    pub fn find_latest_agent_feedback(&self) -> Option<AgentFeedback> {
        let mut runs_dirs = Vec::new();
        let root_runs = self.project_root.join(".kvist/runs");
        if root_runs.exists() {
            runs_dirs.push(root_runs);
        }

        // Also check child component directories under src/
        let src_dir = self.project_root.join("src");
        if let Ok(entries) = fs::read_dir(&src_dir) {
            for entry in entries.flatten() {
                let comp_runs = entry.path().join(".kvist/runs");
                if comp_runs.exists() {
                    runs_dirs.push(comp_runs);
                }
            }
        }

        // Find the newest .trajectory.jsonl file
        let mut newest_file: Option<(PathBuf, std::time::SystemTime)> = None;
        for runs_dir in &runs_dirs {
            if let Ok(entries) = fs::read_dir(runs_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
                        && let Ok(meta) = path.metadata()
                        && let Ok(modified) = meta.modified()
                        && newest_file
                            .as_ref()
                            .is_none_or(|(_, best)| modified > *best)
                    {
                        newest_file = Some((path, modified));
                    }
                }
            }
        }

        let (traj_path, _) = newest_file?;
        let report = agent_runtime::replay_trajectory(&traj_path, None).ok()?;
        let mut reasoning = None;
        let mut tokens_in = None;
        let mut tokens_out = None;

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
                } => {
                    tokens_in = Some(tokens_in.unwrap_or(0) + (*t as usize));
                }
                agent_runtime::TrajectoryEvent::TurnFinish {
                    output_tokens: Some(t),
                    ..
                } => {
                    tokens_out = Some(tokens_out.unwrap_or(0) + (*t as usize));
                }
                agent_runtime::TrajectoryEvent::SessionFinish { total_tokens, .. }
                    if tokens_in.is_none() && tokens_out.is_none() =>
                {
                    tokens_out = Some(*total_tokens as usize);
                }
                _ => {}
            }
        }

        // Check for matching .json run record
        if let Some(parent) = traj_path.parent() {
            let json_name = traj_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.replace(".trajectory.jsonl", ".json"));
            if let Some(jname) = json_name {
                let json_path = parent.join(jname);
                if json_path.exists()
                    && let Ok(contents) = fs::read_to_string(&json_path)
                    && let Ok(v) = serde_json::from_str::<serde_json::Value>(&contents)
                {
                    if let Some(in_t) = v.get("tokens_input").and_then(|t| t.as_u64()) {
                        tokens_in = Some(in_t as usize);
                    }
                    if let Some(out_t) = v.get("tokens_output").and_then(|t| t.as_u64()) {
                        tokens_out = Some(out_t as usize);
                    }
                }
            }
        }

        // Find the latest log file in .kvist/logs/
        let mut log_path = None;
        let logs_dir = self.project_root.join(".kvist/logs");
        if logs_dir.exists()
            && let Some(task_id) = &report.task_id
            && let Ok(entries) = fs::read_dir(&logs_dir)
        {
            let mut newest_log = None;
            for entry in entries.flatten() {
                let path = entry.path();
                let fname = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                if fname.starts_with(task_id)
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
            task_id: report.task_id,
            turns: report.replayed_turns,
            tool_calls: report.tool_calls_count,
            tokens_input: tokens_in,
            tokens_output: tokens_out,
            reasoning,
            trajectory_path: Some(traj_path),
            log_path,
        })
    }

    /// Prints the styled Agent Result and Feedback stage.
    pub fn print_result_stage(
        &self,
        result: &std::result::Result<cli::CommandOutput, crate::KvistError>,
    ) {
        println!("╰──────────────────────────────────────────────────────────────────");
        println!("╭── Agent Result ──────────────────────────────────────────────────");
        match result {
            Ok(output) => {
                let text = output.to_string();
                let trimmed = text.trim();
                for line in trimmed.lines() {
                    println!("│  {line}");
                }
                if let Some(feedback) = self.find_latest_agent_feedback() {
                    println!("│");
                    println!("│  Agent Feedback:");
                    if let (Some(in_tok), Some(out_tok)) =
                        (feedback.tokens_input, feedback.tokens_output)
                    {
                        let total = in_tok + out_tok;
                        println!(
                            "│    • Tokens: {in_tok} input · {out_tok} output ({total} total)"
                        );
                    }
                    if feedback.turns > 0 || feedback.tool_calls > 0 {
                        println!(
                            "│    • Activity: {} turn(s), {} tool call(s)",
                            feedback.turns, feedback.tool_calls
                        );
                    }
                    if let Some(reasoning) = &feedback.reasoning {
                        let snippet = if reasoning.len() > 80 {
                            format!("{}...", &reasoning[..77])
                        } else {
                            reasoning.clone()
                        };
                        println!("│    • Reasoning: \"{snippet}\"");
                    }
                    if let Some(traj) = &feedback.trajectory_path
                        && let Ok(rel) = traj.strip_prefix(&self.project_root)
                    {
                        println!("│    • Trajectory: ./{rel}", rel = rel.display());
                    }
                    if let Some(log) = &feedback.log_path
                        && let Ok(rel) = log.strip_prefix(&self.project_root)
                    {
                        println!("│    • Full log: ./{rel}", rel = rel.display());
                    }
                }
            }
            Err(error) => {
                println!("│  ✘ Execution failed: {error}");
            }
        }
        println!("╰──────────────────────────────────────────────────────────────────");
    }

    /// Replaces transient progress display with the finalized command output and log link.
    pub fn finish_stream(
        &self,
        _program: &str,
        result: &std::result::Result<cli::CommandOutput, crate::KvistError>,
        _prompt_line: &str,
    ) {
        self.print_result_stage(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn spinner_starts_and_stops_cleanly() {
        let spinner = ProgressSpinner::start("Testing".into());
        thread::sleep(Duration::from_millis(150));
        spinner.stop();
    }

    #[test]
    fn prepare_log_path_creates_directory_and_returns_valid_path() {
        let dir = tempdir().unwrap();
        let manager = StreamManager::new(dir.path());
        let log_path = manager.prepare_log_path("task-run").unwrap();
        assert!(log_path.starts_with(dir.path().join(".kvist/logs")));
        assert!(log_path.to_str().unwrap().contains("task_run"));
    }

    #[test]
    fn stage_printing_executes_without_panicking() {
        let dir = tempdir().unwrap();
        let manager = StreamManager::new(dir.path());
        manager.print_prompt_stage("task run . implement-code");
        manager.print_working_stage();
        manager.print_result_stage(&Ok(cli::CommandOutput::message("success")));
        manager.print_result_stage(&Err(KvistError::AgentRuntime(
            agent_runtime::Error::Cancelled,
        )));
    }

    #[test]
    fn find_latest_agent_feedback_extracts_metrics() {
        let dir = tempdir().unwrap();
        let runs_dir = dir.path().join(".kvist").join("runs");
        fs::create_dir_all(&runs_dir).unwrap();

        let traj_path = runs_dir.join("task-1_2026-09-11T22-00-00Z.trajectory.jsonl");
        let journal = r#"{"event":"session_start","session_id":"s1","task_id":"task-1","timestamp":1700000000}
{"event":"turn_start","turn":1,"timestamp":1700000001}
{"event":"prompt_eval","turn":1,"new_tokens":50,"cached_tokens":100}
{"event":"model_reasoning","turn":1,"reasoning":"Inspecting test suite."}
{"event":"tool_dispatch","turn":1,"call_id":"c1","tool":"read_file","args":{},"action_hash":"h1"}
{"event":"tool_result","turn":1,"call_id":"c1","tool":"read_file","stdout":"","stderr":"","exit_code":0,"bytes":0,"state_mutated":false}
{"event":"turn_finish","turn":1,"output_tokens":25,"finish_reason":"stop"}
{"event":"session_finish","session_id":"s1","task_id":"task-1","total_turns":1,"total_tokens":75,"success":true}
"#;
        fs::write(&traj_path, journal).unwrap();

        let manager = StreamManager::new(dir.path());
        let feedback = manager
            .find_latest_agent_feedback()
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
    }
}
