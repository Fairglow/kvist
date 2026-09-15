//! Kvist Interactive Workspace Shell implementation.
//!
//! The shell is a reedline line editor hosting a Kvist-specific completer.
//! Static completion (verbs, flags, enum literals) is derived from the same
//! clap command surface the parser uses, plus the shell builtins; dynamic
//! completion (component paths, task IDs, attempt IDs, model names, the
//! active branch) is resolved in memory from a [`DynamicState`] snapshot that
//! is refreshed after every command so completions always track the durable
//! project state.
//!
//! **Commands as Primary:** Standard user inputs are parsed directly as commands
//! (e.g. `task run`, `component validate`). No artificial prefix (like `/` or `:`)
//! is required.
//!
//! **Builtins:** `cd`, `tasks`, `run`, `help`, `last`, `history`, `journal`,
//! `locks`, and `exit`/`quit` are dispatched before a line is re-parsed by
//! clap. `cd` remembers the current component for the builtins, the prompt,
//! and completion ordering; `run` without a task executes the next ready task.
//!
//! **Prompt Authoring:** Raw prompt authoring is an explicit action: typing
//! `prompt <task_id>` opens an external editor (respecting `$VISUAL`/`$EDITOR`)
//! seeded with the task's context, then validates, displays, and submits the
//! result only after explicit confirmation.
//!
//! **Output Streaming & Link Replacement:** During sandboxed model execution,
//! transient progress is displayed on stderr alongside a progress spinner.
//! Once execution completes, ANSI escape sequences clear the raw progress line,
//! replacing it with:
//! 1. The prompt block
//! 2. A clickable link to the raw execution log (`Logs: ./.kvist/logs/...`)
//! 3. The clean, finalized command result
//!
//! **Terminal Output Pagination:** All output is passed through a pager wrapper
//! using the `minus` crate, ensuring large component trees, long logs, and complex
//! status tables are paginated cleanly without buffer overflows or color loss.
//!
//! **Session Journal:** The shell maintains an append-only session journal at
//! `.kvist/session.log` that records final command inputs, execution
//! results, and log links. Transient progress states are excluded. The editor
//! history is reedline's file-backed history at `.kvist/history`.

mod completion;
mod journal;
mod locks;
mod pager;
mod prompt_editor;
mod runs;
mod state;
mod status;
mod stream;
mod style;
mod tree;

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Component as PathComponent, Path, PathBuf};
use std::sync::{Arc, Mutex};

use reedline::{
    DefaultHinter, DefaultPrompt, DefaultPromptSegment, Emacs, IdeMenu, KeyCode, KeyModifiers,
    MenuBuilder, Reedline, ReedlineEvent, ReedlineMenu, Signal, default_emacs_keybindings,
};

use clap::Parser;

use crate::{
    KvistError, Result, cli, task_commands,
    task_queue::{self, TaskKind, TaskStatus},
};
use agent_runtime::split_raw_command;

use completion::KvistCompleter;
pub use journal::{JournalEntry, SESSION_JOURNAL, SessionJournal, display_session_status};
pub use pager::display_output;
pub use prompt_editor::{
    build_prompt_seed, edit_prompt_with_seed, print_prompt_block, prompt_editor_command,
};
pub use runs::RecentRun;
pub use state::DynamicState;
pub use status::{StatusContext, print_welcome_banner, short_prompt, status_bar_label};
pub use stream::{AgentFeedback, ProgressSpinner, StreamManager};
pub use style::{Theme, report_error};
use tree::build_root;

/// Name of the completion menu registered with the line editor.
const COMPLETION_MENU: &str = "kvist_completion";

/// Truncates text to a maximum character count, adding an ellipsis if needed.
pub(crate) fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_owned()
    } else {
        let mut truncated: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        truncated.push_str("...");
        truncated
    }
}

/// The outcome of handling one shell line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopAction {
    /// Keep the REPL running.
    Continue,
    /// Leave the REPL.
    Exit,
}

/// Builds a journal entry for a command line and a result summary.
fn journal_entry(command: &str, result: &str) -> JournalEntry {
    JournalEntry {
        timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        command: command.to_owned(),
        result: result.to_owned(),
    }
}

/// One interactive session: the project root, its append-only journal, the
/// stream manager, the resolved theme, the current-component focus shared
/// with completion, and the last command's exit state for the prompt.
pub(crate) struct Shell {
    project_dir: PathBuf,
    journal: SessionJournal,
    stream_manager: StreamManager,
    theme: Theme,
    /// The current component (via `cd`), shared with the completer so it can
    /// be reordered without rebuilding the line editor.
    focus: Arc<Mutex<Option<String>>>,
    /// Whether the last executed command failed; the next prompt shows the
    /// failure marker until a command succeeds.
    last_failed: bool,
}

impl Shell {
    /// Creates a shell session for one project directory.
    pub(crate) fn new(project_dir: &Path, focus: Arc<Mutex<Option<String>>>, theme: Theme) -> Self {
        Self {
            project_dir: project_dir.to_path_buf(),
            journal: SessionJournal::new(project_dir),
            stream_manager: StreamManager::new(project_dir, theme),
            theme,
            focus,
            last_failed: false,
        }
    }

    /// Records the exit state of the last handled line for the prompt.
    fn note(&mut self, failed: bool) {
        self.last_failed = failed;
    }

    /// The current component focus, when one is set.
    pub(crate) fn current_component(&self) -> Option<String> {
        self.focus.lock().ok().and_then(|guard| guard.clone())
    }

    /// Handles one shell line, never returning an error: every failure mode
    /// is reported to the user and the session continues.
    pub(crate) fn handle_line(
        &mut self,
        line: &str,
        history: &dyn reedline::History,
    ) -> LoopAction {
        let line = line.trim();
        if line.is_empty() {
            return LoopAction::Continue;
        }
        if line == "exit" || line == "quit" {
            return LoopAction::Exit;
        }

        let (program, arguments) = match split_raw_command(line) {
            Ok(split) => split,
            Err(error) => {
                report_error(self.theme, &error.to_string());
                self.journal.append(journal_entry(line, "syntax error"));
                self.note(true);
                return LoopAction::Continue;
            }
        };

        match program.as_str() {
            "journal" => {
                display_session_status(self.theme, &self.journal);
                self.journal.append(journal_entry(line, "shown"));
                self.note(false);
            }
            "history" => {
                self.handle_history(&arguments, line, history);
            }
            "help" => {
                display_output(&render_help(self.theme));
                self.journal.append(journal_entry(line, "shown"));
                self.note(false);
            }
            "cd" => {
                self.handle_cd(&arguments, line);
            }
            "tasks" => {
                self.handle_tasks(&arguments, line);
            }
            "run" => {
                self.handle_run(&arguments, line);
            }
            "last" => {
                self.handle_last(&arguments, line);
            }
            "locks" if arguments.is_empty() || arguments == ["clean"] => {
                handle_locks(self.theme, !arguments.is_empty(), &self.journal);
                self.note(false);
            }
            "locks" => {
                report_error(self.theme, "usage: locks [clean]");
                self.journal.append(journal_entry(line, "usage hint"));
                self.note(true);
            }
            // In the interactive shell REPL, bare 'status' and 'overview'
            // display the human-friendly project overview; anything with
            // arguments is the real CLI command.
            "status" | "overview" if arguments.is_empty() => {
                self.handle_overview(line);
            }
            // `prompt` is an explicit, high-signal authoring action and never
            // falls through to the CLI parser (an unknown task must not run
            // as prompt text).
            "prompt" => match arguments.len() {
                0 => {
                    report_error(
                        self.theme,
                        "usage: prompt <TASK_ID> — opens your editor seeded with the task context",
                    );
                    self.journal.append(journal_entry(line, "usage hint"));
                    self.note(true);
                }
                1 => {
                    self.handle_prompt_authoring(line, &arguments[0]);
                }
                _ => {
                    self.dispatch(line);
                }
            },
            _ => {
                self.dispatch(line);
            }
        }
        LoopAction::Continue
    }

    /// Shows the human-friendly project overview for bare `status`/`overview`.
    fn handle_overview(&mut self, line: &str) {
        let result = (|| {
            let inspection = crate::project_state::inspect(&self.project_dir)?;
            Ok::<String, KvistError>(crate::status::render(
                &inspection,
                crate::status::StatusFormat::Overview,
                false,
                false,
                false,
            ))
        })();
        match result {
            Ok(text) => {
                display_output(&text);
                self.journal
                    .append(journal_entry(line, &truncate(&text, 100)));
                self.note(false);
            }
            Err(error) => {
                let _ = error.print();
                self.journal.append(journal_entry(line, "error"));
                self.note(true);
            }
        }
    }

    /// `cd [COMPONENT_DIR]`: remember the current component for the builtins,
    /// the prompt, and completion ordering.
    fn handle_cd(&mut self, args: &[String], line: &str) -> LoopAction {
        let known = DynamicState::load(&self.project_dir).components().to_vec();
        match resolve_cd(args, &known) {
            CdOutcome::Show => {
                let current = self
                    .current_component()
                    .filter(|c| !c.is_empty() && c != ".")
                    .unwrap_or_else(|| ". (root)".to_owned());
                println!("Component: {}", self.theme.bold(&current));
                self.journal.append(journal_entry(line, "show"));
                self.note(false);
            }
            CdOutcome::Set(component) => {
                if let Ok(mut guard) = self.focus.lock() {
                    *guard = Some(component.clone());
                }
                println!("Component: {}", self.theme.bold(&component));
                self.journal.append(journal_entry(line, &component));
                self.note(false);
            }
            CdOutcome::Unknown { requested, known } => {
                if known.is_empty() {
                    report_error(self.theme, "no components discovered in this project");
                } else {
                    report_error(
                        self.theme,
                        &format!(
                            "unknown component `{requested}`; known: {}",
                            known.join(", ")
                        ),
                    );
                }
                self.journal
                    .append(journal_entry(line, "unknown component"));
                self.note(true);
            }
            CdOutcome::Invalid(requested) => {
                report_error(
                    self.theme,
                    &format!(
                        "`{requested}` is not a valid component path (use a relative path or `.`)"
                    ),
                );
                self.journal
                    .append(journal_entry(line, "invalid component path"));
                self.note(true);
            }
            CdOutcome::Usage => {
                report_error(self.theme, "usage: cd [COMPONENT_DIR]");
                self.journal.append(journal_entry(line, "usage hint"));
                self.note(true);
            }
        }
        LoopAction::Continue
    }

    /// `tasks [COMPONENT_DIR] [--status STATUS]`: a compact task table.
    fn handle_tasks(&mut self, args: &[String], line: &str) -> LoopAction {
        let parsed = match parse_tasks_args(args) {
            Ok(parsed) => parsed,
            Err(message) => {
                report_error(self.theme, &message);
                self.journal.append(journal_entry(line, "usage hint"));
                self.note(true);
                return LoopAction::Continue;
            }
        };
        let hint = parsed.component.as_deref().and_then(|requested| {
            component_focus_hint(self.current_component().as_deref(), requested)
        });
        let component = parsed
            .component
            .or_else(|| self.current_component())
            .unwrap_or_else(|| ".".to_owned());
        if let Some(hint) = hint {
            println!("{}", self.theme.dim(&hint));
        }
        let state = DynamicState::load(&self.project_dir);
        if !state.components().iter().any(|c| c == &component) {
            let known = state.components().join(", ");
            if known.is_empty() {
                report_error(self.theme, "no components discovered in this project");
            } else {
                report_error(
                    self.theme,
                    &format!("unknown component `{component}`; known: {known}"),
                );
            }
            self.journal
                .append(journal_entry(line, "unknown component"));
            self.note(true);
            return LoopAction::Continue;
        }
        let Some(scope) = state.scopes.get(&component) else {
            println!("No readable task queue in `{component}`.");
            self.journal.append(journal_entry(line, "no queue"));
            self.note(false);
            return LoopAction::Continue;
        };
        let next_ready = task_queue::next_ready_task_id(&scope.tasks);
        let rows: Vec<TaskRow> = scope
            .tasks
            .iter()
            .filter(|task| parsed.status.is_none_or(|s| s == task.status))
            .map(|task| TaskRow {
                id: task.id.clone(),
                status: task.status,
                kind: task.kind,
                title: task.title.clone(),
                blocked_reason: task.blocked_reason.clone(),
            })
            .collect();
        display_output(&render_tasks(
            self.theme,
            &component,
            &rows,
            next_ready.as_deref(),
        ));
        self.journal
            .append(journal_entry(line, &format!("{} tasks", rows.len())));
        self.note(false);
        LoopAction::Continue
    }

    /// `run [COMPONENT_DIR] [TASK_ID]`: run a task, or — when the task is
    /// omitted — suggest the next ready task. An omitted task is never
    /// auto-executed: the next ready task runs only after an explicit
    /// confirmation (bare ENTER accepts).
    fn handle_run(&mut self, args: &[String], line: &str) -> LoopAction {
        let (requested_component, task) = match parse_run_args(args) {
            Ok(parsed) => parsed,
            Err(message) => {
                report_error(self.theme, &message);
                self.journal.append(journal_entry(line, "usage hint"));
                self.note(true);
                return LoopAction::Continue;
            }
        };
        let hint = requested_component.as_deref().and_then(|requested| {
            component_focus_hint(self.current_component().as_deref(), requested)
        });
        let component = requested_component
            .or_else(|| self.current_component())
            .unwrap_or_else(|| ".".to_owned());
        if let Some(hint) = hint {
            println!("{}", self.theme.dim(&hint));
        }
        let suggested = task.is_none();
        let task = match task {
            Some(task) => task,
            None => {
                match resolve_next_ready_task(&DynamicState::load(&self.project_dir), &component) {
                    Some(task_id) => task_id,
                    None => {
                        report_error(
                            self.theme,
                            &format!(
                                "no ready tasks in `{component}`; pass a TASK_ID or inspect `tasks`"
                            ),
                        );
                        self.journal.append(journal_entry(line, "no ready task"));
                        self.note(true);
                        return LoopAction::Continue;
                    }
                }
            }
        };
        if suggested && !self.confirm_suggested_run(&task, line) {
            return LoopAction::Continue;
        }
        self.dispatch_labeled(&format!("task run {component} {task}"), line);
        LoopAction::Continue
    }

    /// Asks the user to confirm a suggested (next-ready) run. A refusal, or a
    /// confirmation that cannot be obtained in a non-interactive context, aborts
    /// the run without changing durable state.
    fn confirm_suggested_run(&mut self, task_id: &str, line: &str) -> bool {
        match task_commands::confirm_run_suggestion(task_id) {
            Ok(confirmed) => {
                if !confirmed {
                    println!("Cancelled: did not run the suggested task `{task_id}`.");
                    self.journal.append(journal_entry(line, "run cancelled"));
                    self.note(false);
                }
                confirmed
            }
            Err(error) => {
                let _ = error.print();
                self.journal
                    .append(journal_entry(line, "confirmation unavailable"));
                self.note(true);
                false
            }
        }
    }

    /// `last [COUNT]`: recent agent runs with tokens and log links.
    fn handle_last(&mut self, args: &[String], line: &str) -> LoopAction {
        let count = match parse_count("last", args, 10) {
            Ok(count) => count,
            Err(message) => {
                report_error(self.theme, &message);
                self.journal.append(journal_entry(line, "usage hint"));
                self.note(true);
                return LoopAction::Continue;
            }
        };
        let runs = runs::scan_recent_runs(&self.project_dir, count);
        display_output(&render_recent_runs(self.theme, &runs, &self.project_dir));
        self.journal
            .append(journal_entry(line, &format!("{} runs", runs.len())));
        self.note(false);
        LoopAction::Continue
    }

    /// `history [COUNT]`: the editor's own history (what up-arrow recalls).
    fn handle_history(
        &mut self,
        args: &[String],
        line: &str,
        history: &dyn reedline::History,
    ) -> LoopAction {
        let count = match parse_count("history", args, 20) {
            Ok(count) => count,
            Err(message) => {
                report_error(self.theme, &message);
                self.journal.append(journal_entry(line, "usage hint"));
                self.note(true);
                return LoopAction::Continue;
            }
        };
        let query = reedline::SearchQuery {
            direction: reedline::SearchDirection::Backward,
            start_time: None,
            end_time: None,
            start_id: None,
            end_id: None,
            limit: Some(count as i64),
            filter: reedline::SearchFilter::anything(None),
        };
        match history.search(query) {
            Ok(items) if items.is_empty() => {
                println!("Editor history: (empty)");
                self.journal.append(journal_entry(line, "empty"));
                self.note(false);
            }
            Ok(items) => {
                let mut text = format!("Editor history ({} most recent):\n", items.len());
                for (i, item) in items.iter().enumerate() {
                    text.push_str(&format!(
                        "  {}. {}\n",
                        self.theme.dim(&format!("{i}")),
                        item.command_line
                    ));
                }
                display_output(&text);
                self.journal
                    .append(journal_entry(line, &format!("{} lines", items.len())));
                self.note(false);
            }
            Err(error) => {
                report_error(
                    self.theme,
                    &format!("could not read editor history: {error}"),
                );
                self.journal.append(journal_entry(line, "history error"));
                self.note(true);
            }
        }
        LoopAction::Continue
    }

    /// Runs the `prompt <task_id>` editor flow without ever killing the
    /// session; submission requires explicit confirmation.
    fn handle_prompt_authoring(&mut self, line: &str, task_id: &str) -> LoopAction {
        let command = match prompt_editor_command(&self.project_dir, task_id) {
            Ok(Some(command)) => command,
            Ok(None) => {
                report_error(
                    self.theme,
                    &format!(
                        "no such task `{task_id}`. Try `tasks` to list tasks or `overview` for project status."
                    ),
                );
                self.journal.append(journal_entry(line, "no such task"));
                self.note(true);
                return LoopAction::Continue;
            }
            Err(error) => {
                let _ = error.print();
                self.journal.append(journal_entry(line, "editor error"));
                self.note(true);
                return LoopAction::Continue;
            }
        };

        // The prompt block is already displayed; running the agent still
        // needs explicit consent.
        if !confirm("Submit this prompt and start the agent run? [y/N] ") {
            println!("Prompt submission cancelled.");
            self.journal.append(journal_entry(line, "prompt cancelled"));
            self.note(false);
            return LoopAction::Continue;
        }

        let spinner = self.stream_manager.start_spinner("authoring prompt");
        self.stream_manager.print_prompt_stage(line);
        self.stream_manager.print_working_stage();
        let result = cli::execute(command, false);
        spinner.stop();
        // A free prompt run records no trajectory, so there is nothing to correlate.
        self.stream_manager
            .print_result_stage(&result, &stream::FeedbackTarget::None);

        let summary = match &result {
            Ok(out) => truncate(&out.to_string(), 100),
            Err(err) => truncate(&err.to_string(), 100),
        };
        self.journal.append(journal_entry(line, &summary));
        self.note(result.is_err());
        LoopAction::Continue
    }

    /// Dispatches a command line through the CLI, handling streaming,
    /// paging, confirmation gates, and journaling. Total: every failure is
    /// reported and the session continues.
    fn dispatch(&mut self, line: &str) {
        self.dispatch_labeled(line, line);
    }

    /// Dispatches `command_line` while journaling `journal_line` (used by
    /// builtins that synthesize a CLI command, e.g. `run` → `task run`).
    fn dispatch_labeled(&mut self, command_line: &str, journal_line: &str) {
        let (program, arguments) = match split_raw_command(command_line) {
            Ok(split) => split,
            Err(error) => {
                report_error(self.theme, &error.to_string());
                self.journal
                    .append(journal_entry(journal_line, "syntax error"));
                self.note(true);
                return;
            }
        };

        let mut full_args: Vec<String> = vec!["kvist".to_owned(), program.clone()];
        full_args.extend(arguments.iter().cloned());

        let parsed = match cli::Cli::try_parse_from(&full_args) {
            Ok(parsed) => parsed,
            Err(error) => {
                let _ = error.print();
                self.journal
                    .append(journal_entry(journal_line, "syntax error"));
                self.note(true);
                return;
            }
        };

        if matches!(parsed.command, cli::Command::Shell(_)) {
            println!("You are already in an active Kvist shell.");
            self.journal
                .append(journal_entry(journal_line, "already in shell"));
            self.note(false);
            return;
        }

        // Destructive operations require an explicit in-shell confirmation.
        if let Some(gate) = destructive_gate(&parsed.command)
            && !confirm(&format!("confirm {gate}? [y/N] "))
        {
            println!("Operation cancelled.");
            self.journal
                .append(journal_entry(journal_line, "confirmation declined"));
            self.note(false);
            return;
        }

        let streaming = is_streaming_command(&parsed.command);
        let interactive = is_interactive_command(&parsed.command);
        // Capture the run's task before moving the command, for feedback
        // correlation with the run's own trajectory.
        let run_task_id = match &parsed.command {
            cli::Command::Task {
                command: cli::TaskCommand::Run { task_id, .. },
            } => task_id.clone(),
            _ => None,
        };
        let mut cmd = parsed.command;
        if streaming {
            if let cli::Command::Task {
                command: cli::TaskCommand::Run { ref mut stream, .. },
            } = cmd
            {
                *stream = true;
            }
            self.stream_manager.print_prompt_stage(command_line);
            self.stream_manager.print_working_stage();
        }

        // A spinner with elapsed time covers slow background work so it is
        // visibly progressing rather than hung. Interactive commands drive their
        // own prompt (a wizard here) and must not be covered by a spinner that
        // would imply a background agent run while the command waits for input.
        let spinner = if interactive {
            None
        } else {
            Some(self.stream_manager.start_spinner(&program))
        };
        let result = cli::execute(cmd, false);
        if let Some(spinner) = spinner {
            spinner.stop();
        }

        if streaming {
            let feedback_target = match run_task_id.as_deref() {
                Some(task_id) => stream::FeedbackTarget::Task(task_id),
                None => stream::FeedbackTarget::None,
            };
            self.stream_manager
                .print_result_stage(&result, &feedback_target);
        } else {
            match &result {
                Ok(output) => display_output(&output.to_string()),
                Err(error) => {
                    if matches!(
                        error,
                        KvistError::AgentSetupCancelled
                            | KvistError::AgentRuntime(agent_runtime::Error::Cancelled)
                    ) {
                        println!("Operation cancelled.");
                    } else {
                        let _ = error.print();
                    }
                }
            }
        }

        let summary = match &result {
            Ok(out) => truncate(&out.to_string(), 100),
            Err(err) => truncate(&err.to_string(), 100),
        };
        self.journal.append(journal_entry(journal_line, &summary));
        // A cooperative cancellation is not a failure for the prompt state.
        self.note(matches!(
            &result,
            Err(error)
                if !matches!(
                    error,
                    KvistError::AgentSetupCancelled
                        | KvistError::AgentRuntime(agent_runtime::Error::Cancelled)
                )
        ));
    }
}

/// Lists live and stale task locks; `locks clean` removes the stale ones.
fn handle_locks(theme: Theme, clean: bool, journal: &SessionJournal) -> LoopAction {
    let command = if clean { "locks clean" } else { "locks" };
    let entries = locks::scan();
    let cleaned = if clean {
        locks::clean_stale()
    } else {
        Vec::new()
    };
    if entries.is_empty() {
        println!("No task locks.");
        journal.append(journal_entry(command, "none"));
        return LoopAction::Continue;
    }
    display_output(&render_locks(theme, &entries, &cleaned, clean));
    journal.append(journal_entry(
        command,
        &format!("{} locks, {} cleaned", entries.len(), cleaned.len()),
    ));
    LoopAction::Continue
}

/// Renders the task-lock listing (pure, so it is testable without user state).
fn render_locks(
    theme: Theme,
    entries: &[locks::LockEntry],
    cleaned: &[PathBuf],
    clean: bool,
) -> String {
    let mut text = format!(
        "{}\n",
        theme.bold(&format!("Task locks ({}):", entries.len()))
    );
    for entry in entries {
        let state = if entry.live {
            theme.green("[live]")
        } else {
            theme.red("[stale]")
        };
        let task = entry.task_id.as_deref().unwrap_or("<unknown>");
        let pid = entry
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "<unknown>".to_owned());
        let age = locks::format_age(entry.age_secs);
        text.push_str(&format!(
            "  {state} task {task} · pid {pid} · {}\n",
            theme.dim(&format!("age {age}"))
        ));
    }
    for path in cleaned {
        text.push_str(&format!(
            "  {}\n",
            theme.dim(&format!("cleaned: {}", path.display()))
        ));
    }
    if clean && cleaned.is_empty() {
        text.push_str(&format!("  {}\n", theme.dim("no stale locks to clean")));
    }
    text
}

/// The outcome of resolving the `cd` builtin arguments.
enum CdOutcome {
    /// No argument: show the current component.
    Show,
    /// A discovered component to make current.
    Set(String),
    /// The argument is not a discovered component.
    Unknown {
        requested: String,
        known: Vec<String>,
    },
    /// The argument is not a usable component path.
    Invalid(String),
    /// Too many arguments.
    Usage,
}

/// Resolves `cd` arguments against the discovered component set (pure).
fn resolve_cd(args: &[String], known: &[String]) -> CdOutcome {
    if args.len() > 1 {
        return CdOutcome::Usage;
    }
    let Some(arg) = args.first() else {
        return CdOutcome::Show;
    };
    if arg != "."
        && (Path::new(arg).is_absolute()
            || Path::new(arg)
                .components()
                .any(|c| matches!(c, PathComponent::ParentDir | PathComponent::CurDir)))
    {
        return CdOutcome::Invalid(arg.clone());
    }
    if known.iter().any(|c| c == arg) {
        CdOutcome::Set(arg.clone())
    } else {
        CdOutcome::Unknown {
            requested: arg.clone(),
            known: known.to_vec(),
        }
    }
}

/// One row of the `tasks` builtin table.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskRow {
    id: String,
    status: TaskStatus,
    kind: TaskKind,
    title: String,
    blocked_reason: Option<String>,
}

/// The CLI spelling of a durable task status.
fn status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pending => "pending",
        TaskStatus::InProgress => "in-progress",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Completed => "completed",
    }
}

/// The CLI spelling of a task lifecycle role.
fn kind_label(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::Test => "test",
        TaskKind::Implementation => "implementation",
        TaskKind::SecurityAudit => "security-audit",
        TaskKind::ComplianceReview => "compliance-review",
    }
}

/// Parses a CLI status literal into a durable task status.
fn parse_task_status(value: &str) -> std::result::Result<TaskStatus, String> {
    match value {
        "pending" => Ok(TaskStatus::Pending),
        "in-progress" => Ok(TaskStatus::InProgress),
        "blocked" => Ok(TaskStatus::Blocked),
        "completed" => Ok(TaskStatus::Completed),
        other => Err(format!(
            "unknown status `{other}`; expected pending, in-progress, blocked, or completed"
        )),
    }
}

/// Parsed `tasks` builtin arguments.
struct TasksArgs {
    component: Option<String>,
    status: Option<TaskStatus>,
}

/// Parses `tasks [COMPONENT_DIR] [--status STATUS]` (pure).
fn parse_tasks_args(args: &[String]) -> std::result::Result<TasksArgs, String> {
    let mut component = None;
    let mut status = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--status" {
            let Some(value) = args.get(i + 1) else {
                return Err(
                    "--status needs a value: pending, in-progress, blocked, or completed"
                        .to_owned(),
                );
            };
            status = Some(parse_task_status(value)?);
            i += 2;
        } else if let Some(value) = arg.strip_prefix("--status=") {
            status = Some(parse_task_status(value)?);
            i += 1;
        } else {
            if component.is_some() {
                return Err("usage: tasks [COMPONENT_DIR] [--status STATUS]".to_owned());
            }
            component = Some(arg.clone());
            i += 1;
        }
    }
    Ok(TasksArgs { component, status })
}

/// Parsed `run` builtin arguments: the optional component and task.
type RunArgs = (Option<String>, Option<String>);

/// Parses `run [COMPONENT_DIR] [TASK_ID]` (pure).
fn parse_run_args(args: &[String]) -> std::result::Result<RunArgs, String> {
    if args.len() > 2 {
        return Err("usage: run [COMPONENT_DIR] [TASK_ID]".to_owned());
    }
    Ok((args.first().cloned(), args.get(1).cloned()))
}

/// Resolves the next ready task (the first pending task whose recorded
/// dependencies are all completed) for a component from a snapshot, or
/// `None` when the component has no readable queue or no task is ready.
fn resolve_next_ready_task(state: &DynamicState, component: &str) -> Option<String> {
    let scope = state.scopes.get(component)?;
    task_queue::next_ready_task_id(&scope.tasks)
}

/// Builds a hint for when a command names a component that is not the current
/// focus, so the user is never surprised by which component a later bare
/// command will target. Returns `None` when the requested component matches the
/// focus (root and an unset focus compare equal). Pure: it never changes focus.
fn component_focus_hint(current: Option<&str>, requested: &str) -> Option<String> {
    let requested_focus = if requested.is_empty() || requested == "." {
        None
    } else {
        Some(requested)
    };
    let current_focus = match current {
        Some(c) if !c.is_empty() && c != "." => Some(c),
        _ => None,
    };
    if requested_focus == current_focus {
        return None;
    }
    Some(match current_focus {
        None => format!("note: no component focused (cd {requested} to switch)"),
        Some(current) => format!("note: still focused on {current} (cd {requested} to switch)"),
    })
}

/// Parses a `last [COUNT]` / `history [COUNT]` argument (pure).
fn parse_count(
    command: &str,
    args: &[String],
    default: usize,
) -> std::result::Result<usize, String> {
    match args.len() {
        0 => Ok(default),
        1 => match args[0].parse::<usize>() {
            Ok(0) => Ok(default),
            Ok(count) => Ok(count),
            Err(_) => Err(format!(
                "`{command}` expects a positive integer count, got `{}`",
                args[0]
            )),
        },
        _ => Err(format!("usage: {command} [COUNT]")),
    }
}

/// The colored spelling of a durable task status in the `tasks` table.
fn status_style(theme: Theme, status: TaskStatus) -> String {
    match status {
        TaskStatus::Pending => theme.dim(status_label(status)),
        TaskStatus::InProgress => theme.cyan(status_label(status)),
        TaskStatus::Blocked => theme.red(status_label(status)),
        TaskStatus::Completed => theme.green(status_label(status)),
    }
}

/// Renders the `tasks` table (pure, so it is testable without a terminal).
fn render_tasks(
    theme: Theme,
    component: &str,
    rows: &[TaskRow],
    next_ready: Option<&str>,
) -> String {
    if rows.is_empty() {
        return format!("No tasks in `{component}` match the filter.");
    }
    let mut text = format!(
        "{}\n",
        theme.bold(&format!("Tasks in `{component}` ({} shown):", rows.len()))
    );
    for row in rows {
        let marker = if next_ready.is_some_and(|n| n == row.id) {
            theme.yellow("★ ")
        } else {
            "  ".to_owned()
        };
        text.push_str(&format!(
            "{}{:<28} {} {:<16} {}\n",
            style::pad_right(&marker, 2),
            truncate(&row.id, 28),
            style::pad_right(&status_style(theme, row.status), 12),
            kind_label(row.kind),
            truncate(&row.title, 60)
        ));
        if let Some(reason) = &row.blocked_reason {
            text.push_str(&format!(
                "   {}\n",
                theme.dim(&format!("blocked: {}", truncate(reason, 100)))
            ));
        }
    }
    if let Some(next) = next_ready {
        text.push_str(&format!(
            "{} {}\n",
            theme.dim("next ready:"),
            theme.bold(next)
        ));
    }
    text
}

/// Renders the `last` builtin run table (pure, so it is testable).
fn render_recent_runs(theme: Theme, runs: &[RecentRun], project_root: &Path) -> String {
    if runs.is_empty() {
        return "No agent runs recorded yet.".to_owned();
    }
    let mut text = format!(
        "{}\n",
        theme.bold(&format!("Recent agent runs ({}):", runs.len()))
    );
    for run in runs {
        let component = run.component.as_deref().unwrap_or(".");
        let task = run.task_id.as_deref().unwrap_or("<unknown>");
        let status = match run.success {
            Some(true) => theme.green("success"),
            Some(false) => theme.red("failed"),
            None => theme.dim("unknown"),
        };
        let tokens = match (run.tokens_input, run.tokens_output) {
            (Some(input), Some(output)) => format!("{input}/{output}"),
            _ => "-".to_owned(),
        };
        let timestamp = theme.dim(run.timestamp.as_deref().unwrap_or("-"));
        text.push_str(&format!(
            "  {task:<28} {component:<14} {} {} {tokens:<12} {timestamp}\n",
            style::pad_right(&status, 8),
            theme.dim("tokens")
        ));
        let link = run.record_path.as_ref().or(run.trajectory_path.as_ref());
        if let Some(path) = link
            && let Ok(rel) = path.strip_prefix(project_root)
        {
            let kind = if run.record_path.as_ref() == Some(path) {
                "record"
            } else {
                "trajectory"
            };
            text.push_str(&format!(
                "    {}\n",
                theme.dim(&format!("{kind}: ./{rel}", rel = rel.display()))
            ));
        }
    }
    text
}

/// Renders the `help` builtin text (pure, so it is testable).
fn render_help(theme: Theme) -> String {
    let mut text = String::new();
    text.push_str(&format!("{}\n", theme.bold("Kvist shell — builtins:")));
    text.push_str(
        "  cd [COMPONENT]                 set the current component for builtins and completion\n",
    );
    text.push_str("  tasks [COMPONENT] [--status S] list tasks; S: pending, in-progress, blocked, completed\n");
    text.push_str(
        "  run [COMPONENT] [TASK]         run a task; omit TASK to confirm the next ready one\n",
    );
    text.push_str("  last [COUNT]                   recent agent runs with tokens and log links\n");
    text.push_str("  history [COUNT]                recent editor history lines\n");
    text.push_str("  journal                        the append-only session journal\n");
    text.push_str("  locks [clean]                  inspect live/stale task locks; clean removes stale ones\n");
    text.push_str("  prompt TASK                    author a prompt in your editor for a task\n");
    text.push_str("  status | overview              human-friendly project overview\n");
    text.push_str("  help                           this list\n");
    text.push_str("  exit | quit                    leave the shell\n");
    text.push_str(
        "\nKey bindings: Tab complete · arrows navigate · Ctrl+C cancel command · Ctrl+D exit\n",
    );
    text.push_str("\nTop workflow commands (the full CLI surface also parses):\n");
    text.push_str("  task next .            show the next ready task\n");
    text.push_str("  task run . [TASK]      run a task; omit TASK to confirm the next ready one\n");
    text.push_str("  task log . TASK        inspect an execution log\n");
    text.push_str("  task transition . TASK STATUS  record a status transition\n");
    text.push_str("  component validate DIR validate component intent documents\n");
    text.push_str("  agent profile list     configured model profiles\n");
    text.push_str("  tree                   the component tree\n");
    text
}

/// Names the destructive part of a command that needs an in-shell
/// confirmation, when the command is in the declared destructive set.
fn destructive_gate(command: &cli::Command) -> Option<&'static str> {
    match command {
        cli::Command::Task {
            command: cli::TaskCommand::Unlock { force: true, .. },
        } => Some("task unlock --force (releases a component lock)"),
        cli::Command::Agent {
            command: cli::AgentCommand::Remove { all: true, .. },
        } => Some("agent remove --all (removes every model profile)"),
        cli::Command::Component {
            command: cli::ComponentCommand::Accept { commit: true, .. },
        } => Some("component accept --commit (creates a Git commit)"),
        cli::Command::Vcs {
            command: cli::VcsCommand::CommitAccepted { .. },
        } => Some("vcs commit-accepted (creates an acceptance commit)"),
        _ => None,
    }
}

/// Asks the user to confirm an operation; a missing, unreadable, or cancelled
/// answer is a refusal, so confirmation can never block, hang, or crash the
/// session. Ctrl-C while the prompt is displayed interrupts the read and is
/// treated as a refusal.
fn confirm(prompt: &str) -> bool {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    let mut reader = crate::interruptible_stdin::interruptible_reader();
    match reader.read_line(&mut answer) {
        Ok(0) | Err(_) => false,
        Ok(_) => matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
    }
}

/// Returns whether a command type produces long-running output that should be streamed.
/// Commands that drive their own interactive prompt and therefore must not be
/// covered by the progress spinner (see `dispatch_labeled`).
fn is_interactive_command(command: &cli::Command) -> bool {
    matches!(
        command,
        cli::Command::Agent {
            command: cli::AgentCommand::Setup { .. }
                | cli::AgentCommand::Profile {
                    command: Some(cli::AgentProfileCommand::Add { .. })
                }
        }
    )
}

fn is_streaming_command(command: &cli::Command) -> bool {
    match command {
        cli::Command::Task { command } => matches!(
            command,
            cli::TaskCommand::Run { .. } | cli::TaskCommand::Recover { .. }
        ),
        cli::Command::Prompt {
            detect_loops,
            max_restarts,
            ..
        } => *detect_loops || *max_restarts > 0,
        _ => false,
    }
}

/// Builds the line editor with the Kvist completer, inline hinter, IDE
/// completion menu, and file-backed history at `.kvist/history`.
fn build_editor(
    completer: Box<KvistCompleter>,
    project_dir: &Path,
) -> std::result::Result<Reedline, KvistError> {
    let completion_menu = Box::new(
        IdeMenu::default()
            .with_name(COMPLETION_MENU)
            .with_default_border(),
    );
    let mut keybindings = default_emacs_keybindings();

    // Tab opens the completion menu or cycles to next candidate
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu(COMPLETION_MENU.to_owned()),
            ReedlineEvent::MenuNext,
        ]),
    );

    // Ctrl+Space forces the completion menu on demand. `with_quick_completions`
    // already opens the menu after a few characters, so this lets the user open
    // or reopen it earlier and at will. It is the same convention fish and
    // zsh+fzf use; the `>` marker + reverse highlight (reedline defaults)
    // identify the selected candidate.
    keybindings.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char(' '),
        ReedlineEvent::Menu(COMPLETION_MENU.to_owned()),
    );

    // Shift-Tab cycles to previous candidate
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );
    keybindings.add_binding(
        KeyModifiers::SHIFT,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );

    // Down arrow navigates menu when open, else standard Down
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Down,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::MenuDown,
            ReedlineEvent::MenuNext,
            ReedlineEvent::Down,
        ]),
    );

    // Up arrow navigates menu when open, else standard Up
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Up,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::MenuUp,
            ReedlineEvent::MenuPrevious,
            ReedlineEvent::Up,
        ]),
    );

    // Left and Right arrow keys inside menu
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Right,
        ReedlineEvent::UntilFound(vec![ReedlineEvent::MenuRight, ReedlineEvent::Right]),
    );
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Left,
        ReedlineEvent::UntilFound(vec![ReedlineEvent::MenuLeft, ReedlineEvent::Left]),
    );

    // Esc closes the completion menu
    keybindings.add_binding(KeyModifiers::NONE, KeyCode::Esc, ReedlineEvent::Esc);

    // Ctrl+L clears the screen and redraws the prompt (reedline ships no
    // default binding for it).
    keybindings.add_binding(
        KeyModifiers::CONTROL,
        KeyCode::Char('l'),
        ReedlineEvent::ClearScreen,
    );

    let edit_mode = Box::new(Emacs::new(keybindings));

    // Persistent editor history; a history the editor cannot open must not
    // prevent the shell from starting, so degrade to in-memory history.
    let history: Box<dyn reedline::History> = match reedline::FileBackedHistory::with_file(
        reedline::HISTORY_SIZE,
        project_dir.join(".kvist").join("history"),
    ) {
        Ok(history) => Box::new(history),
        Err(error) => {
            eprintln!("warning: persistent history unavailable ({error}); using in-memory history");
            Box::new(reedline::FileBackedHistory::default())
        }
    };

    let editor = Reedline::create()
        .with_completer(completer)
        .with_history(history)
        .with_hinter(Box::new(DefaultHinter::default()))
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode)
        .with_quick_completions(true);
    Ok(editor)
}

/// Maximum consecutive terminal read failures before the shell exits.
const MAX_CONSECUTIVE_READ_FAILURES: u32 = 3;

/// Launches and runs the persistent interactive workspace shell (REPL).
pub fn run_shell(project_dir: &Path) -> Result<()> {
    // The line editor requires an interactive terminal; refuse early with an
    // actionable diagnostic instead of dying inside reedline's read loop.
    if !std::io::stdin().is_terminal() {
        return Err(KvistError::ShellNotInteractive {
            reason: "standard input is not a terminal (e.g. it is a pipe); run `kvist shell` from an interactive terminal, or use the regular CLI commands for scripts".to_owned(),
        });
    }

    // Install the shared SIGINT/SIGTERM handler once for the process.
    agent_runtime::install_handler();

    // Resolve the theme once for the session: NO_COLOR, CLICOLOR,
    // CLICOLOR_FORCE, TERM=dumb, and terminal detection are all honored, and
    // the result degrades to plain text.
    let theme = Theme::detect();

    let root = build_root();
    let state = Arc::new(Mutex::new(DynamicState::load(project_dir)));
    let focus = Arc::new(Mutex::new(None));
    let mut shell = Shell::new(project_dir, focus.clone(), theme);
    let completer = Box::new(KvistCompleter::new(root, state.clone(), focus));
    let status = StatusContext::load(project_dir);

    let mut editor = build_editor(completer, project_dir)?;

    let initial_branch = state
        .lock()
        .ok()
        .and_then(|guard| guard.branch().map(str::to_owned));
    print_welcome_banner(
        theme,
        &status,
        initial_branch.as_deref(),
        shell.current_component().as_deref(),
    );

    let mut consecutive_read_failures = 0_u32;
    loop {
        // Keep dynamic completions in step with the durable project state.
        if let Ok(mut guard) = state.lock() {
            *guard = DynamicState::load(project_dir);
        }

        let current_status = StatusContext::load(project_dir);
        let branch = state
            .lock()
            .ok()
            .and_then(|guard| guard.branch().map(str::to_owned));
        let prompt = DefaultPrompt {
            left_prompt: DefaultPromptSegment::Basic(short_prompt(
                theme,
                branch.as_deref(),
                shell.current_component().as_deref(),
                shell.last_failed,
            )),
            right_prompt: DefaultPromptSegment::Basic(status_bar_label(theme, &current_status)),
        };

        let signal = match editor.read_line(&prompt) {
            Ok(signal) => {
                consecutive_read_failures = 0;
                signal
            }
            Err(error) => {
                // A transient terminal glitch (resize storm, lost cursor
                // query, broken pipe) must not kill the session: retry a few
                // times, then exit with an actionable diagnostic.
                consecutive_read_failures += 1;
                if consecutive_read_failures >= MAX_CONSECUTIVE_READ_FAILURES {
                    eprintln!("error: the terminal stopped responding (last error: {error})");
                    eprintln!(
                        "hint: check the terminal size and kill hung child processes, then run `kvist shell` again"
                    );
                    break;
                }
                eprintln!(
                    "warning: could not read a line ({error}); retrying ({consecutive_read_failures}/{MAX_CONSECUTIVE_READ_FAILURES})..."
                );
                continue;
            }
        };

        match signal {
            Signal::CtrlC => {
                println!("^C");
                // A SIGINT delivered while the prompt was displayed is the
                // same event; the `^C` echo is the feedback, so consume the
                // flag silently instead of misreporting it on the next command.
                let _ = agent_runtime::take_interrupted();
            }
            Signal::CtrlD => {
                println!();
                break;
            }
            Signal::Success(line) => {
                if shell.handle_line(&line, editor.history()) == LoopAction::Exit {
                    break;
                }
                // An interrupt arriving while a command ran is consumed by the
                // command's supervision; if it still lingers, the command did
                // not handle it and simply finished on its own.
                if agent_runtime::take_interrupted() {
                    eprintln!(
                        "note: an interrupt was received while a command was running; it finished normally — re-run it if the result looks stale"
                    );
                }
            }
            // `Signal` is `#[non_exhaustive]`: future reedline releases can add
            // variants (for example `HostCommand` or `ExternalBreak`). Match the
            // ones Kvist acts on explicitly and treat anything else as a no-op so
            // a library release can never break the shell with an exhaustiveness
            // error.
            _ => {}
        }
    }

    // The journal is append-only on disk; nothing is buffered in memory.
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    fn test_shell(dir: &Path) -> (Shell, reedline::FileBackedHistory) {
        (
            Shell::new(dir, Arc::new(Mutex::new(None)), Theme::plain()),
            reedline::FileBackedHistory::default(),
        )
    }

    #[test]
    fn truncate_helper() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(
            truncate("a very long line of text exceeding limit", 10),
            "a very ..."
        );
        // Multi-byte text must not panic on a byte boundary.
        let multibyte = "é".repeat(100);
        assert_eq!(truncate(&multibyte, 10).chars().count(), 10);
    }

    #[test]
    fn is_streaming_command_classifies_correctly() {
        let task_run = cli::Command::Task {
            command: cli::TaskCommand::Run {
                component_dir: ".".into(),
                task_id: Some("t1".to_owned()),
                stream: true,
            },
        };
        assert!(is_streaming_command(&task_run));

        let task_next = cli::Command::Task {
            command: cli::TaskCommand::Next {
                component_dir: ".".into(),
            },
        };
        assert!(!is_streaming_command(&task_next));

        let tree = cli::Command::Tree(cli::ProjectDirectory { path: ".".into() });
        assert!(!is_streaming_command(&tree));

        let agent_list = cli::Command::Agent {
            command: cli::AgentCommand::List,
        };
        assert!(!is_streaming_command(&agent_list));
    }

    #[test]
    fn dispatch_handles_unknown_command_gracefully() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        shell.handle_line("unknown-command", &history);
        assert_eq!(shell.journal.entries().len(), 1);
        assert_eq!(shell.journal.entries()[0].result, "syntax error");
    }

    #[test]
    fn dispatch_handles_shell_recursion() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        shell.handle_line("shell", &history);
        assert_eq!(shell.journal.entries()[0].result, "already in shell");
    }

    #[test]
    fn handle_line_exits_on_exit_and_quit() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        assert_eq!(shell.handle_line("exit", &history), LoopAction::Exit);
        assert_eq!(shell.handle_line("quit", &history), LoopAction::Exit);
    }

    #[test]
    fn handle_line_continues_on_empty_lines() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        assert_eq!(shell.handle_line("", &history), LoopAction::Continue);
        assert_eq!(shell.handle_line("   ", &history), LoopAction::Continue);
    }

    #[test]
    fn handle_line_routes_journal_builtin() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        assert_eq!(shell.handle_line("journal", &history), LoopAction::Continue);
        assert_eq!(shell.journal.entries()[0].result, "shown");
    }

    #[test]
    fn handle_line_history_builtin_reads_editor_history() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        assert_eq!(shell.handle_line("history", &history), LoopAction::Continue);
        assert_eq!(shell.journal.entries()[0].result, "empty");
    }

    #[test]
    fn handle_line_rejects_unknown_prompt_task_instead_of_falling_through() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        assert_eq!(
            shell.handle_line("prompt no-such-task", &history),
            LoopAction::Continue
        );
        assert_eq!(shell.journal.entries().len(), 1);
        assert_eq!(shell.journal.entries()[0].result, "no such task");
    }

    #[test]
    fn handle_line_shows_prompt_usage_when_no_task_given() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        assert_eq!(shell.handle_line("prompt", &history), LoopAction::Continue);
        assert_eq!(shell.journal.entries()[0].result, "usage hint");
    }

    #[test]
    fn handle_line_never_propagates_unparseable_lines() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        assert_eq!(shell.handle_line("task", &history), LoopAction::Continue);
        assert_eq!(shell.journal.entries().len(), 1);
        assert_eq!(shell.journal.entries()[0].result, "syntax error");
    }

    #[test]
    fn last_builtin_reports_no_runs_for_a_bare_directory() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        assert_eq!(shell.handle_line("last", &history), LoopAction::Continue);
        assert_eq!(shell.journal.entries()[0].result, "0 runs");
    }

    #[test]
    fn cd_builtin_without_components_reports_discovery_failure() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        assert_eq!(
            shell.handle_line("cd engine", &history),
            LoopAction::Continue
        );
        assert_eq!(shell.journal.entries()[0].result, "unknown component");
    }

    // ---- Pure builtin logic -------------------------------------------------

    #[test]
    fn resolve_cd_handles_show_set_unknown_and_invalid() {
        let known = vec![".".to_owned(), "engine".to_owned()];
        assert!(matches!(resolve_cd(&[], &known), CdOutcome::Show));
        assert!(matches!(
            resolve_cd(&["engine".to_owned()], &known),
            CdOutcome::Set(c) if c == "engine"
        ));
        assert!(matches!(
            resolve_cd(&["ghost".to_owned()], &known),
            CdOutcome::Unknown { ref known, .. } if known.len() == 2
        ));
        assert!(matches!(
            resolve_cd(&["/abs".to_owned()], &known),
            CdOutcome::Invalid(_)
        ));
        assert!(matches!(
            resolve_cd(&["..".to_owned()], &known),
            CdOutcome::Invalid(_)
        ));
        assert!(matches!(
            resolve_cd(&["a".to_owned(), "b".to_owned()], &known),
            CdOutcome::Usage
        ));
    }

    #[test]
    fn parse_tasks_args_accepts_flag_and_positional_in_any_order() {
        let args = parse_tasks_args(&[
            "--status".to_owned(),
            "blocked".to_owned(),
            "engine".to_owned(),
        ])
        .expect("valid args");
        assert_eq!(args.component.as_deref(), Some("engine"));
        assert_eq!(args.status, Some(TaskStatus::Blocked));

        let args = parse_tasks_args(&["engine".to_owned(), "--status=pending".to_owned()])
            .expect("valid args");
        assert_eq!(args.component.as_deref(), Some("engine"));
        assert_eq!(args.status, Some(TaskStatus::Pending));

        assert!(parse_tasks_args(&["a".to_owned(), "b".to_owned()]).is_err());
        assert!(parse_tasks_args(&["--status".to_owned()]).is_err());
        assert!(parse_tasks_args(&["--status".to_owned(), "nope".to_owned()]).is_err());
    }

    #[test]
    fn parse_run_args_accepts_optional_component_and_task() {
        assert_eq!(parse_run_args(&[]).expect("valid"), (None, None));
        assert_eq!(
            parse_run_args(&["engine".to_owned()]).expect("valid"),
            (Some("engine".to_owned()), None)
        );
        assert_eq!(
            parse_run_args(&["engine".to_owned(), "t-1".to_owned()]).expect("valid"),
            (Some("engine".to_owned()), Some("t-1".to_owned()))
        );
        assert!(parse_run_args(&["a".to_owned(), "b".to_owned(), "c".to_owned()]).is_err());
    }

    fn ready_task(id: &str, status: TaskStatus, deps: &[&str]) -> task_queue::Task {
        task_queue::Task {
            id: id.to_owned(),
            title: id.to_owned(),
            description: String::new(),
            context: String::new(),
            purpose: String::new(),
            expected_outcome: String::new(),
            kind: TaskKind::Test,
            status,
            depends_on: deps.iter().map(|dep| (*dep).to_owned()).collect(),
            requirements: Vec::new(),
            timestamps: task_queue::TaskTimestamps {
                created_at: task_queue::Timestamp::default(),
                updated_at: task_queue::Timestamp::default(),
                completed_at: None,
            },
            blocked_reason: None,
            recovery_state: None,
            acceptance_id: None,
            disposition: None,
        }
    }

    fn state_with_scope(component: &str, tasks: Vec<task_queue::Task>) -> DynamicState {
        let mut scopes = std::collections::BTreeMap::new();
        scopes.insert(
            component.to_owned(),
            state::ComponentScope {
                tasks,
                attempts: std::collections::BTreeMap::new(),
            },
        );
        DynamicState::new(vec![component.to_owned()], scopes, Vec::new(), None)
    }

    #[test]
    fn resolve_next_ready_task_selects_the_first_ready_task() {
        // `later` is listed first but waits on `dep`; `dep` is the ready task.
        let state = state_with_scope(
            ".",
            vec![
                ready_task("later", TaskStatus::Pending, &["dep"]),
                ready_task("dep", TaskStatus::Pending, &[]),
            ],
        );
        assert_eq!(resolve_next_ready_task(&state, "."), Some("dep".to_owned()));

        // Once `dep` completes, `later` becomes the ready task.
        let state = state_with_scope(
            ".",
            vec![
                ready_task("later", TaskStatus::Pending, &["dep"]),
                ready_task("dep", TaskStatus::Completed, &[]),
            ],
        );
        assert_eq!(
            resolve_next_ready_task(&state, "."),
            Some("later".to_owned())
        );

        // A blocked queue has no ready task.
        let state = state_with_scope(".", vec![ready_task("blocked", TaskStatus::Blocked, &[])]);
        assert_eq!(resolve_next_ready_task(&state, "."), None);
        // An unknown component has no scope.
        assert_eq!(resolve_next_ready_task(&state, "ghost"), None);
    }

    #[test]
    fn component_focus_hint_only_when_requested_differs_from_focus() {
        // No focus: naming a component suggests focusing it.
        assert_eq!(
            component_focus_hint(None, "engine"),
            Some("note: no component focused (cd engine to switch)".to_owned())
        );
        // A root request with no focus matches: no hint.
        assert_eq!(component_focus_hint(None, "."), None);
        // A request matching the focus: no hint.
        assert_eq!(component_focus_hint(Some("engine"), "engine"), None);
        // A different component than the focus: hint.
        assert_eq!(
            component_focus_hint(Some("engine"), "agent_runtime"),
            Some("note: still focused on engine (cd agent_runtime to switch)".to_owned())
        );
        // Requesting root while focused elsewhere: hint.
        assert_eq!(
            component_focus_hint(Some("engine"), "."),
            Some("note: still focused on engine (cd . to switch)".to_owned())
        );
        // A root focus compares equal to no focus.
        assert_eq!(
            component_focus_hint(Some("."), "engine"),
            Some("note: no component focused (cd engine to switch)".to_owned())
        );
    }

    #[test]
    fn run_builtin_without_a_ready_task_reports_and_continues() {
        let dir = tempdir().unwrap();
        let (mut shell, history) = test_shell(dir.path());
        assert_eq!(shell.handle_line("run", &history), LoopAction::Continue);
        assert_eq!(shell.journal.entries()[0].result, "no ready task");
    }

    #[test]
    fn parse_count_defaults_and_validates() {
        assert_eq!(parse_count("last", &[], 10).expect("default"), 10);
        assert_eq!(
            parse_count("last", &["0".to_owned()], 10).expect("zero"),
            10
        );
        assert_eq!(
            parse_count("last", &["3".to_owned()], 10).expect("count"),
            3
        );
        assert!(parse_count("last", &["x".to_owned()], 10).is_err());
        assert!(parse_count("last", &["1".to_owned(), "2".to_owned()], 10).is_err());
    }

    fn row(id: &str, status: TaskStatus) -> TaskRow {
        TaskRow {
            id: id.to_owned(),
            status,
            kind: TaskKind::Test,
            title: format!("title {id}"),
            blocked_reason: None,
        }
    }

    #[test]
    fn render_tasks_marks_the_next_ready_task() {
        let rows = vec![
            row("a", TaskStatus::Completed),
            row("b", TaskStatus::Pending),
        ];
        let text = render_tasks(Theme::plain(), "engine", &rows, Some("b"));
        assert!(text.starts_with("Tasks in `engine` (2 shown):\n"));
        assert!(text.contains("★ b"));
        assert!(text.contains("next ready: b"));

        let blocked = vec![TaskRow {
            id: "c".to_owned(),
            status: TaskStatus::Blocked,
            kind: TaskKind::Implementation,
            title: "t".to_owned(),
            blocked_reason: Some("waiting on x".to_owned()),
        }];
        let text = render_tasks(Theme::plain(), ".", &blocked, None);
        assert!(text.contains("implementation"));
        assert!(text.contains("blocked: waiting on x"));
        assert!(render_tasks(Theme::plain(), "engine", &[], None).contains("match the filter"));

        // A colored theme keeps every line at the same visible width as the
        // plain rendering, so styled cells never drift out of their columns.
        let colored = Theme::enabled();
        let plain_text = render_tasks(Theme::plain(), "engine", &rows, Some("b"));
        let styled_text = render_tasks(colored, "engine", &rows, Some("b"));
        for (plain, styled) in plain_text.lines().zip(styled_text.lines()) {
            assert_eq!(
                style::visible_len(styled),
                style::visible_len(plain),
                "column drift in {styled:?}"
            );
        }
        let a_row = styled_text
            .lines()
            .find(|line| line.contains("completed"))
            .expect("completed row present");
        assert!(a_row.contains("\x1b[32m"), "completed row should be green");
        let b_row = styled_text
            .lines()
            .find(|line| line.contains("pending"))
            .expect("pending row present");
        assert!(b_row.contains("\x1b[2m"), "pending row should be dim");
        let star_row = styled_text
            .lines()
            .find(|line| line.contains('★'))
            .expect("next-ready marker present");
        assert!(
            star_row.contains("\x1b[33m"),
            "next-ready marker should be yellow"
        );
    }

    #[test]
    fn render_recent_runs_renders_status_tokens_and_links() {
        let dir = tempdir().unwrap();
        let record = dir.path().join(".kvist/runs/t-1_2026-09-11T22-00-00Z.json");
        fs::create_dir_all(record.parent().unwrap()).unwrap();
        fs::write(&record, "{}").unwrap();
        let runs = vec![RecentRun {
            component: Some("engine".to_owned()),
            task_id: Some("t-1".to_owned()),
            timestamp: Some("2026-09-11T22-00-00Z".to_owned()),
            success: Some(true),
            tokens_input: Some(100),
            tokens_output: Some(50),
            record_path: Some(record.clone()),
            trajectory_path: None,
        }];
        let text = render_recent_runs(Theme::plain(), &runs, dir.path());
        assert!(text.contains("Recent agent runs (1):"));
        assert!(text.contains("t-1"));
        assert!(text.contains("success"));
        assert!(text.contains("100/50"));
        assert!(text.contains("record: ./.kvist/runs/t-1_2026-09-11T22-00-00Z.json"));
        assert_eq!(
            render_recent_runs(Theme::plain(), &[], dir.path()),
            "No agent runs recorded yet."
        );
    }

    #[test]
    fn render_help_lists_every_builtin() {
        let text = render_help(Theme::plain());
        for name in [
            "cd", "tasks", "run", "last", "history", "journal", "locks", "prompt", "status",
            "help", "exit",
        ] {
            assert!(text.contains(name), "help missing {name}");
        }
    }

    #[test]
    fn destructive_gate_covers_the_declared_destructive_set() {
        let unlock = cli::Command::Task {
            command: cli::TaskCommand::Unlock {
                component_dir: ".".into(),
                force: true,
            },
        };
        assert!(destructive_gate(&unlock).is_some());
        let unlock_soft = cli::Command::Task {
            command: cli::TaskCommand::Unlock {
                component_dir: ".".into(),
                force: false,
            },
        };
        assert!(destructive_gate(&unlock_soft).is_none());
        let remove_all = cli::Command::Agent {
            command: cli::AgentCommand::Remove {
                name: None,
                all: true,
                global: false,
            },
        };
        assert!(destructive_gate(&remove_all).is_some());
        let accept_commit = cli::Command::Component {
            command: cli::ComponentCommand::Accept {
                component_dir: ".".into(),
                commit: true,
                message: None,
            },
        };
        assert!(destructive_gate(&accept_commit).is_some());
        let commit_accepted = cli::Command::Vcs {
            command: cli::VcsCommand::CommitAccepted {
                acceptance_id: "a1".to_owned(),
            },
        };
        assert!(destructive_gate(&commit_accepted).is_some());
        let plain_run = cli::Command::Task {
            command: cli::TaskCommand::Run {
                component_dir: ".".into(),
                task_id: Some("t1".to_owned()),
                stream: false,
            },
        };
        assert!(destructive_gate(&plain_run).is_none());
    }

    fn lock_entry(live: bool, task: Option<&str>) -> locks::LockEntry {
        locks::LockEntry {
            path: PathBuf::from(format!("/state/kvist/task-locks-v1/{task:?}.lock")),
            task_id: task.map(str::to_owned),
            pid: Some(4242),
            live,
            age_secs: Some(95),
        }
    }

    #[test]
    fn render_locks_lists_states_and_cleaned_paths() {
        let entries = vec![
            lock_entry(true, Some("write-tests")),
            lock_entry(false, None),
        ];
        let text = render_locks(Theme::plain(), &entries, &[], false);
        assert!(text.starts_with("Task locks (2):\n"));
        assert!(text.contains("[live] task write-tests · pid 4242 · age 1m"));
        assert!(text.contains("[stale] task <unknown>"));

        let cleaned = vec![PathBuf::from("/state/kvist/task-locks-v1/x.lock")];
        let text = render_locks(Theme::plain(), &entries, &cleaned, true);
        assert!(text.contains("cleaned: /state/kvist/task-locks-v1/x.lock"));

        let text = render_locks(Theme::plain(), &entries, &[], true);
        assert!(text.contains("no stale locks to clean"));

        // Live and stale locks are color-coded when styling is enabled.
        let text = render_locks(Theme::enabled(), &entries, &[], false);
        assert!(text.contains("\x1b[32m[live]\x1b[0m"));
        assert!(text.contains("\x1b[31m[stale]\x1b[0m"));
    }
}
