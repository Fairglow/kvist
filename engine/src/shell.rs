//! Kvist Interactive Workspace Shell implementation.
//!
//! The shell is a reedline line editor hosting a Kvist-specific completer.
//! Static completion (verbs, flags, enum literals) is derived from the same
//! clap command surface the parser uses; dynamic completion (component paths,
//! task IDs, attempt IDs, model names, the active branch) is resolved in
//! memory from a [`DynamicState`] snapshot that is refreshed after every
//! command so completions always track the durable project state.
//!
//! Raw prompt authoring is an explicit action: typing `prompt <task_id>`
//! opens an editor seeded with the task's context, then validates, displays,
//! and submits the result. All other input is parsed directly as a command.
//!
//! **Streaming output:** During sandboxed execution, output is streamed to
//! stderr alongside a transient progress spinner. Once execution completes,
//! ANSI escape sequences clear the raw stream chunks and replace them with:
//! 1. The original prompt block (collapsed or highlighted)
//! 2. A clickable link to the raw execution log (e.g., `Logs: ./.kvist/logs/...`)
//! 3. The clean, finalized command result
//!
//! **Session journal:** The shell maintains an active session journal at
//! `.kvist/session.log` that records only final command inputs, execution
//! results, and log links. Transient stream chunks and progress states are
//! strictly excluded from the permanent journal.

mod completion;
mod state;
mod tree;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reedline::{
    ColumnarMenu, DefaultPrompt, DefaultPromptSegment, Emacs, KeyCode, KeyModifiers, MenuBuilder,
    Reedline, ReedlineEvent, ReedlineMenu, Signal, default_emacs_keybindings,
};

use clap::Parser;

use crate::{KvistError, Result, cli, config};
use agent_runtime::{MAX_PROMPT_BYTES, split_raw_command};

use completion::KvistCompleter;
use state::DynamicState;
use tree::build_root;

/// Name of the completion menu registered with the line editor.
const COMPLETION_MENU: &str = "kvist_completion";

/// Stable context shown in the status bar that does not change per prompt.
#[derive(Debug, Clone)]
struct StatusContext {
    sandbox_backend: String,
    default_model: String,
}

impl StatusContext {
    fn load(project_dir: &Path) -> Self {
        match config::load(project_dir) {
            Ok(cfg) => Self {
                sandbox_backend: cfg
                    .sandbox
                    .as_ref()
                    .map(|sandbox| sandbox.backend.clone())
                    .unwrap_or_else(|| "none".to_owned()),
                default_model: cfg
                    .agent
                    .developer
                    .models
                    .first()
                    .map(|model| model.name.clone())
                    .unwrap_or_else(|| "none".to_owned()),
            },
            Err(_) => Self {
                sandbox_backend: "none".to_owned(),
                default_model: "none".to_owned(),
            },
        }
    }
}

/// Active locks displayed in the status bar.
#[derive(Debug, Default)]
#[allow(dead_code)]
struct ActiveLocks {
    /// Lock paths currently held by writers, keyed by lock file path.
    locks: Mutex<BTreeMap<PathBuf, LockInfo>>,
}

/// Information about an active lock.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LockInfo {
    /// Task ID that holds the lock.
    task_id: String,
    /// When the lock was acquired.
    acquired_at: Instant,
    /// The process or thread that holds it.
    owner: String,
}

#[allow(dead_code)]
impl ActiveLocks {
    /// Records a new active lock.
    pub fn record(&self, path: PathBuf, task_id: String, owner: String) {
        let mut locks = self.locks.lock().unwrap();
        locks.insert(
            path.clone(),
            LockInfo {
                task_id,
                acquired_at: Instant::now(),
                owner,
            },
        );
    }

    /// Removes a lock when released.
    pub fn release(&self, path: &PathBuf) {
        let mut locks = self.locks.lock().unwrap();
        locks.remove(path);
    }

    /// Returns all active locks as a list for display.
    pub fn as_list(&self) -> Vec<(PathBuf, LockInfo)> {
        let locks = self.locks.lock().unwrap();
        locks.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Returns the number of active locks.
    pub fn count(&self) -> usize {
        self.locks.lock().unwrap().len()
    }
}

/// Session journal file path relative to the project root.
const SESSION_JOURNAL: &str = ".kvist/session.log";

/// A single entry in the session journal.
#[derive(Debug)]
struct JournalEntry {
    /// Timestamp of the entry.
    timestamp: String,
    /// The command that was run.
    command: String,
    /// The result or output (brief summary only).
    result: String,
    /// Whether this was a transient stream (not logged).
    transient: bool,
}

impl Clone for JournalEntry {
    fn clone(&self) -> Self {
        Self {
            timestamp: self.timestamp.clone(),
            command: self.command.clone(),
            result: self.result.clone(),
            transient: self.transient,
        }
    }
}

/// Manages the session journal.
struct SessionJournal {
    /// Path to the journal file relative to the project root.
    journal_path: PathBuf,
    /// In-memory cache of entries.
    entries: Mutex<Vec<JournalEntry>>,
}

impl SessionJournal {
    /// Creates a new session journal for the given project root.
    fn new(project_root: &Path) -> Self {
        Self {
            journal_path: project_root.join(SESSION_JOURNAL),
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Appends an entry to the journal.
    fn append(&self, entry: JournalEntry) {
        let mut entries = self.entries.lock().unwrap();
        entries.push(entry);
        // Persist periodically (every 100 entries)
        if entries.len().is_multiple_of(100) {
            self.persist();
        }
    }

    /// Flushes all pending entries to disk.
    fn persist(&self) {
        let entries = self.entries.lock().unwrap().clone();
        if entries.is_empty() {
            return;
        }
        if let Some(parent) = self.journal_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        // Write atomically using a temp file
        let temp_path = format!("{}.tmp", self.journal_path.display());
        let mut content =
            "# Session Journal\n# Format: timestamp|command|result|transient\n".to_string();
        for entry in &entries {
            content.push_str(&format!(
                "{}|{}|{}|{}\n",
                entry.timestamp, entry.command, entry.result, entry.transient
            ));
        }
        if let Err(e) = fs::write(&temp_path, &content) {
            eprintln!("Warning: could not persist session journal: {}", e);
        }
        // Atomic rename
        if fs::rename(&temp_path, &self.journal_path).is_err() {
            // Clean up temp file on failure
            let _ = fs::remove_file(&temp_path);
        }
    }

    /// Flushes pending entries on shutdown.
    fn flush(&self) {
        self.persist();
    }

    /// Returns entries since a given index (for resuming display).
    #[allow(dead_code)]
    pub fn since(&self, since_index: usize) -> Vec<String> {
        let entries = self.entries.lock().unwrap().clone();
        entries
            .iter()
            .skip(since_index)
            .map(|e| e.command.clone())
            .collect()
    }
}

/// Spins a progress indicator on stderr while a long-running command executes.
/// Returns the path to the generated log file.
fn spin(project_root: &Path, duration: Duration, log_path: &Path) -> Result<PathBuf> {
    use std::io::{self, Write};

    let interval = Duration::from_millis(100);
    let start = Instant::now();
    let spinner_chars = ['⠋', '⣽', '⣟', '⣯', '⣷', '⣿', '⣽', '⢿'];
    let mut char_idx = 0;
    let mut _spinner_display = String::from("  ");

    loop {
        if start.elapsed() >= duration {
            break;
        }

        let elapsed = start.elapsed();
        let progress = elapsed.as_secs_f64() / duration.as_secs_f64();
        _spinner_display = format!(
            "  {} {:>5.0}% {}",
            spinner_chars[char_idx % spinner_chars.len()],
            (progress * 100.0) as u32,
            spinner_chars[char_idx % spinner_chars.len()]
        );
        char_idx += 1;

        // Write to stderr without flushing stdout
        let _ = io::stderr().write_all(_spinner_display.as_bytes());
        let _ = io::stderr().flush();

        std::thread::sleep(interval);
    }

    // Clear the spinner
    let _ = writeln!(io::stderr(), "\r  ✓ done");
    let _ = io::stderr().flush();

    // Record the log path for stream replacement
    let log_dir = project_root.join(".kvist/logs");
    fs::create_dir_all(&log_dir).map_err(|e| KvistError::Io {
        operation: "create logs directory",
        path: log_dir.clone(),
        source: e,
    })?;
    let log_path = log_dir.join(
        log_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown"),
    );
    fs::write(&log_path, "").map_err(|e| KvistError::Io {
        operation: "create log file",
        path: log_path.clone(),
        source: e,
    })?;
    Ok(log_path)
}

/// Computes the width of a string in terminal columns (accounts for wide characters).
fn terminal_width(text: &str) -> usize {
    text.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

/// Truncates text to a maximum width, adding ellipsis if needed.
fn truncate(text: &str, max_width: usize) -> String {
    if terminal_width(text) <= max_width {
        text.to_owned()
    } else {
        let mut truncated = String::new();
        for ch in text.chars() {
            if terminal_width(&truncated) + 1 > max_width {
                truncated.push_str("...");
                break;
            }
            truncated.push(ch);
        }
        truncated
    }
}

/// Builds a ReedlineMenu that displays the session journal.
fn build_journal_menu(
    _project_dir: &Path,
    _journal: &SessionJournal,
    _stream_manager: &StreamManager,
) -> ReedlineMenu {
    ReedlineMenu::EngineCompleter(Box::new(ColumnarMenu::default().with_name("kvist_journal")))
}

/// Dispatches a command line, handling streaming and journaling.
fn dispatch(
    line: &str,
    project_dir: &Path,
    journal: &SessionJournal,
    stream_manager: &StreamManager,
) -> Result<()> {
    let (program, arguments) = split_raw_command(line).map_err(KvistError::AgentRuntime)?;

    // `prompt <task_id>` is an explicit, high-signal authoring action.
    if program == "prompt" && arguments.len() == 1 {
        let command = prompt_editor_command(project_dir, &arguments[0])?;
        if let Some(command) = command {
            execute_command(command);
            journal.append(JournalEntry {
                timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                command: format!("prompt {}", arguments[0]),
                result: "prompt authoring initiated".into(),
                transient: false,
            });
            return Ok(());
        }
    }

    let mut full_args: Vec<String> = vec!["kvist".to_owned(), program.clone()];
    full_args.extend(arguments.iter().cloned());

    let program_for_log = program.clone();
    let log_path = project_dir
        .join(".kvist/logs")
        .join(format!("{}.log", program_for_log.replace('-', "_")));

    match cli::Cli::try_parse_from(&full_args) {
        Ok(parsed) => {
            if matches!(parsed.command, cli::Command::Shell(_)) {
                println!("You are already in an active Kvist shell.");
                return Ok(());
            }

            // Start streaming if this is a long-running command
            let should_stream = is_streaming_command(&parsed.command);
            let result = if should_stream {
                stream_manager.start_stream(log_path.clone())?;
                let result = cli::execute(parsed.command, false);
                stream_manager.finish_stream(&format!("{:?}", result), line)?;
                if let Ok(output) = result
                    && !output.is_empty()
                {
                    println!("{}", output);
                }
                format!(
                    "Execution complete. Logs: {}",
                    log_path
                        .strip_prefix(project_dir)
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| log_path.display().to_string())
                )
            } else {
                let result = cli::execute(parsed.command, false);
                format!("{:?}", result)
            };

            if !result.is_empty() {
                println!("{}", result);
            }

            // Record in journal
            journal.append(JournalEntry {
                timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                command: format!(
                    "{} {} {}",
                    program,
                    arguments
                        .iter()
                        .map(|s| format!("\"{}\"", s))
                        .collect::<Vec<_>>()
                        .join(" "),
                    ""
                ),
                result: truncate(&result, 100),
                transient: false,
            });

            Ok(())
        }
        Err(error) => {
            let _ = error.print();
            journal.append(JournalEntry {
                timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                command: line.to_string(),
                result: error.to_string(),
                transient: false,
            });
            Ok(())
        }
    }
}

/// Returns whether a command type produces long-running output that should be streamed.
fn is_streaming_command(command: &cli::Command) -> bool {
    match command {
        cli::Command::Task { .. } => true,
        cli::Command::Component { .. } => true,
        cli::Command::Agent { .. } => true,
        cli::Command::Import { .. } => true,
        cli::Command::Vcs { .. } => true,
        cli::Command::Prompt {
            detect_loops,
            max_restarts,
            ..
        } => *detect_loops || *max_restarts > 0,
        _ => false,
    }
}

/// Builds the line editor with the Kvist completer and a columnar menu.
fn build_editor(
    completer: Box<KvistCompleter>,
    journal_menu: ReedlineMenu,
) -> std::result::Result<Reedline, KvistError> {
    let completion_menu = Box::new(ColumnarMenu::default().with_name(COMPLETION_MENU));
    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu(COMPLETION_MENU.to_owned()),
            ReedlineEvent::MenuNext,
        ]),
    );
    let edit_mode = Box::new(Emacs::new(keybindings));

    let editor = Reedline::create()
        .with_completer(completer)
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_menu(journal_menu)
        .with_edit_mode(edit_mode)
        .with_quick_completions(true);
    Ok(editor)
}

fn prompt_label(status: &StatusContext, branch: Option<&str>) -> String {
    format!(
        "kvist ({}) [sandbox: {}] [model: {}] > ",
        branch.unwrap_or("no-vcs"),
        status.sandbox_backend,
        status.default_model
    )
}

/// Parses one line and either runs the `prompt <task_id>` editor flow or
/// dispatches the line as a normal command.
fn prompt_editor_command(project_dir: &Path, task_id: &str) -> Result<Option<cli::Command>> {
    let state = DynamicState::load(project_dir);
    let Some((component, task)) = state.find_task(task_id) else {
        return Ok(None);
    };

    println!("Authoring a prompt for task `{task_id}` in component `{component}`:");
    let prompt = edit_prompt_with_seed(&build_prompt_seed(task))?;
    print_prompt_block(&prompt);

    Ok(Some(cli::Command::Prompt {
        prompt: Some(prompt),
        file: None,
        editor: false,
        role: "developer".to_owned(),
        model: None,
        reasoning_effort: None,
        idle_timeout: 900,
        detect_loops: false,
        max_restarts: 3,
        allow_host_execution: false,
    }))
}

/// Opens `$VISUAL`/`$EDITOR` (defaulting to `vi`) on a temporary file seeded
/// with `seed`, and returns the validated contents after the editor exits.
fn edit_prompt_with_seed(seed: &str) -> Result<String> {
    let directory = tempfile::tempdir().map_err(|source| KvistError::Io {
        operation: "create prompt editor directory",
        path: std::env::temp_dir(),
        source,
    })?;
    let prompt_path = directory.path().join("prompt.md");
    fs::write(&prompt_path, seed).map_err(|source| KvistError::Io {
        operation: "create prompt editor file",
        path: prompt_path.clone(),
        source,
    })?;

    let editor = std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| "vi".into())
        .into_string()
        .map_err(|_| KvistError::InvalidPromptInput {
            reason: "VISUAL or EDITOR must be valid UTF-8".to_owned(),
        })?;
    let (program, mut arguments) = if Path::new(&editor).is_file() {
        (editor, Vec::new())
    } else {
        split_raw_command(&editor).map_err(KvistError::AgentRuntime)?
    };
    arguments.push(
        prompt_path
            .to_str()
            .ok_or_else(|| KvistError::InvalidPromptInput {
                reason: "temporary prompt path must be valid UTF-8".to_owned(),
            })?
            .to_owned(),
    );

    let status = ProcessCommand::new(&program)
        .args(&arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|source| KvistError::Io {
            operation: "open prompt editor",
            path: PathBuf::from(&program),
            source,
        })?;
    if !status.success() {
        return Err(KvistError::InvalidPromptInput {
            reason: format!("editor `{program}` exited with status {status}"),
        });
    }

    let contents = fs::read_to_string(&prompt_path).map_err(|source| KvistError::Io {
        operation: "read edited prompt",
        path: prompt_path.clone(),
        source,
    })?;
    if contents.len() as u64 > MAX_PROMPT_BYTES {
        return Err(KvistError::InvalidPromptInput {
            reason: format!("edited prompt exceeds the {MAX_PROMPT_BYTES}-byte limit"),
        });
    }
    if contents.trim().is_empty() {
        return Err(KvistError::InvalidPromptInput {
            reason: "edited prompt must not be empty".to_owned(),
        });
    }
    Ok(contents)
}

/// Seeds the editor buffer with the task's recorded context.
fn build_prompt_seed(task: &crate::task_queue::Task) -> String {
    let mut seed = String::new();
    seed.push_str(&format!("# Prompt for task `{}`\n", task.id));
    seed.push_str(&format!("Title: {}\n", task.title));
    if !task.description.is_empty() {
        seed.push_str(&format!("\nDescription:\n{}\n", task.description));
    }
    if !task.context.is_empty() {
        seed.push_str(&format!("\nContext:\n{}\n", task.context));
    }
    if !task.purpose.is_empty() {
        seed.push_str(&format!("\nPurpose:\n{}\n", task.purpose));
    }
    if !task.expected_outcome.is_empty() {
        seed.push_str(&format!("\nExpected outcome:\n{}\n", task.expected_outcome));
    }
    seed.push_str("\n---\nEdit the prompt below and save to submit:\n\n");
    seed
}

/// Displays the authored prompt as a formatted block before submission.
fn print_prompt_block(prompt: &str) {
    println!("--------------------------------------------------");
    println!(" prompt");
    for line in prompt.lines() {
        println!("   {line}");
    }
    println!("--------------------------------------------------");
}

/// Launches and runs the persistent interactive workspace shell (REPL).
pub fn run_shell(project_dir: &Path) -> Result<()> {
    let root = build_root();
    let state = Arc::new(Mutex::new(DynamicState::load(project_dir)));
    let completer = Box::new(KvistCompleter::new(root, state.clone()));
    let status = StatusContext::load(project_dir);
    let journal = SessionJournal::new(project_dir);
    let stream_manager = StreamManager::new(project_dir);

    // Register the journal view command
    let journal_menu = build_journal_menu(project_dir, &journal, &stream_manager);
    let mut editor = build_editor(completer, journal_menu)?;

    println!("==================================================");
    println!(" Kvist Interactive Workspace Shell");
    println!(" Type 'exit' or 'quit' or press Ctrl+D to exit.");
    println!(" Press TAB for completion.");
    println!(" Press Ctrl+J to view the session journal.");
    println!("==================================================");

    loop {
        // Keep dynamic completions in step with the durable project state.
        if let Ok(mut guard) = state.lock() {
            *guard = DynamicState::load(project_dir);
        }

        let branch = state
            .lock()
            .ok()
            .and_then(|guard| guard.branch().map(str::to_owned));
        let prompt = DefaultPrompt {
            left_prompt: DefaultPromptSegment::Basic(prompt_label(&status, branch.as_deref())),
            right_prompt: DefaultPromptSegment::Empty,
        };

        let signal = match editor.read_line(&prompt) {
            Ok(signal) => signal,
            Err(error) => {
                eprintln!("error: line read failure: {error}");
                break;
            }
        };

        match signal {
            Signal::CtrlC => {
                println!("^C");
            }
            Signal::CtrlD => {
                println!();
                break;
            }
            Signal::Success(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if line == "exit" || line == "quit" {
                    break;
                }

                // Handle journal view
                if line == "journal" || line == "history" {
                    display_session_status(project_dir, &journal)?;
                    continue;
                }

                dispatch(line, project_dir, &journal, &stream_manager)?;
            }
        }
    }

    // Flush the session journal on exit
    journal.flush();
    Ok(())
}

/// Executes a parsed command, printing any non-empty output.
fn execute_command(command: cli::Command) {
    match cli::execute(command, false) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{}", output);
            }
        }
        Err(error) => {
            let _ = error.print();
        }
    }
}

/// Displays the current session state.
fn display_session_status(_project_root: &Path, journal: &SessionJournal) -> Result<()> {
    let entries = journal.entries.lock().unwrap().clone();
    if entries.is_empty() {
        println!("Session journal: (empty)");
        return Ok(());
    }

    println!("Session journal:");
    let mut count = 0;
    for entry in &entries {
        if entry.transient {
            continue;
        }
        count += 1;
        println!("  [{}] {}", entry.timestamp, entry.command);
    }
    println!("  Total: {} entries", count);
    Ok(())
}

/// Manages transient output streams for sandboxed execution.
///
/// During execution, output is streamed to stderr alongside a spinner.
/// Once execution completes, the streams are cleared and replaced with
/// a link to the log and the finalized result.
struct StreamManager {
    /// Path to the project root.
    project_root: PathBuf,
    /// Whether we're currently streaming output.
    streaming: Mutex<bool>,
    /// The current command's log path.
    current_log: Mutex<Option<PathBuf>>,
    /// Buffer for the transient stream.
    stream_buffer: Mutex<String>,
}

impl StreamManager {
    /// Creates a new stream manager for the given project root.
    fn new(project_root: &Path) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            streaming: Mutex::new(false),
            current_log: Mutex::new(None),
            stream_buffer: Mutex::new(String::new()),
        }
    }

    /// Starts streaming output for a command.
    pub fn start_stream(&self, log_path: PathBuf) -> Result<PathBuf> {
        let log_path = spin(&self.project_root, Duration::from_secs(5), &log_path)?;
        *self.streaming.lock().unwrap() = true;
        *self.current_log.lock().unwrap() = Some(log_path.clone());
        self.stream_buffer.lock().unwrap().clear();
        Ok(log_path)
    }

    /// Appends a chunk to the transient stream buffer.
    #[allow(dead_code)]
    pub fn append(&self, chunk: &str) -> Result<()> {
        if !*self.streaming.lock().unwrap() {
            return Ok(());
        }
        let mut buffer = self.stream_buffer.lock().unwrap();
        buffer.push_str(chunk);
        Ok(())
    }

    /// Clears the transient stream and replaces it with a log link and result.
    ///
    /// This is called after a command completes successfully.
    pub fn finish_stream(&self, result: &str, prompt: &str) -> Result<()> {
        if !*self.streaming.lock().unwrap() {
            return Ok(());
        }
        let log_path = self.current_log.lock().unwrap().clone();
        let mut buffer = self.stream_buffer.lock().unwrap();

        // Clear the transient stream using ANSI escape sequences
        // This erases lines from the current line up to the beginning
        let erase_lines = 20;
        let erase = format!("\x1b[{}K", erase_lines);
        buffer.clear();
        buffer.push_str(&erase);

        // Write the replacement: prompt, log link, and result
        buffer.push_str(prompt);
        buffer.push_str(result);

        // Write the log link (clickable on supported terminals)
        if let Some(log_path) = log_path {
            let relative_log = log_path
                .strip_prefix(&self.project_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| log_path.display().to_string());
            buffer.push_str(&format!("\n\nLogs: {}\n", relative_log));
        }

        *self.streaming.lock().unwrap() = false;
        *self.current_log.lock().unwrap() = None;
        Ok(())
    }

    /// Returns whether streaming is active.
    #[allow(dead_code)]
    pub fn is_streaming(&self) -> bool {
        *self.streaming.lock().unwrap()
    }

    /// Gets the current log path.
    #[allow(dead_code)]
    pub fn current_log(&self) -> Option<PathBuf> {
        self.current_log.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_queue::{Task, TaskKind, TaskStatus, TaskTimestamps};

    fn task(id: &str, description: &str) -> Task {
        Task {
            id: id.to_owned(),
            title: id.to_owned(),
            description: description.to_owned(),
            context: String::new(),
            purpose: String::new(),
            expected_outcome: String::new(),
            kind: TaskKind::Test,
            status: TaskStatus::Pending,
            depends_on: Vec::new(),
            requirements: Vec::new(),
            timestamps: TaskTimestamps {
                created_at: crate::task_queue::Timestamp::default(),
                updated_at: crate::task_queue::Timestamp::default(),
                completed_at: None,
            },
            blocked_reason: None,
            recovery_state: None,
            acceptance_id: None,
            disposition: None,
        }
    }

    #[test]
    fn prompt_label_shows_branch_and_context() {
        let status = StatusContext {
            sandbox_backend: "bubblewrap".to_owned(),
            default_model: "ollama".to_owned(),
        };
        assert_eq!(
            prompt_label(&status, Some("main")),
            "kvist (main) [sandbox: bubblewrap] [model: ollama] > "
        );
        assert_eq!(
            prompt_label(&status, None),
            "kvist (no-vcs) [sandbox: bubblewrap] [model: ollama] > "
        );
    }

    #[test]
    fn prompt_editor_command_is_none_for_an_unknown_task() {
        let dir = tempfile::tempdir().unwrap();
        assert!(prompt_editor_command(dir.path(), "nope").unwrap().is_none());
    }

    #[test]
    fn seed_includes_the_task_context() {
        let task = task("write-tests", "Define the tests.");
        let seed = build_prompt_seed(&task);
        assert!(seed.contains("# Prompt for task `write-tests`"));
        assert!(seed.contains("Title: write-tests"));
        assert!(seed.contains("Define the tests."));
        assert!(seed.ends_with("Edit the prompt below and save to submit:\n\n"));
    }

    #[test]
    fn seed_omits_empty_fields() {
        let task = task("write-tests", "");
        let seed = build_prompt_seed(&task);
        assert!(!seed.contains("Description:"));
        assert!(!seed.contains("Context:"));
    }
}
