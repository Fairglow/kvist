//! Kvist Interactive Workspace Shell implementation.
//!
//! The shell is a reedline line editor hosting a Kvist-specific completer.
//! Static completion (verbs, flags, enum literals) is derived from the same
//! clap command surface the parser uses; dynamic completion (component paths,
//! task IDs, attempt IDs, model names, the active branch) is resolved in
//! memory from a [`DynamicState`] snapshot that is refreshed after every
//! command so completions always track the durable project state.
//!
//! **Commands as Primary:** Standard user inputs are parsed directly as commands
//! (e.g. `task run`, `component validate`). No artificial prefix (like `/` or `:`)
//! is required.
//!
//! **Prompt Authoring:** Raw prompt authoring is an explicit action: typing
//! `prompt <task_id>` opens an external editor (respecting `$VISUAL`/`$EDITOR`)
//! seeded with the task's context, then validates, displays, and submits the result.
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
//! **Session Journal:** The shell maintains an active session journal at
//! `.kvist/session.log` that records only final command inputs, execution
//! results, and log links. Transient progress states are excluded.

mod completion;
mod journal;
mod locks;
mod pager;
mod prompt_editor;
mod state;
mod status;
mod stream;
mod tree;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use reedline::{
    DefaultHinter, DefaultPrompt, DefaultPromptSegment, Emacs, IdeMenu, KeyCode, KeyModifiers,
    MenuBuilder, Reedline, ReedlineEvent, ReedlineMenu, Signal, default_emacs_keybindings,
};

use clap::Parser;

use crate::{KvistError, Result, cli};
use agent_runtime::split_raw_command;

use completion::KvistCompleter;
pub use journal::{JournalEntry, SESSION_JOURNAL, SessionJournal, display_session_status};
pub use pager::display_output;
pub use prompt_editor::{
    build_prompt_seed, edit_prompt_with_seed, print_prompt_block, prompt_editor_command,
};
pub use state::DynamicState;
pub use status::{StatusContext, print_welcome_banner, short_prompt, status_bar_label};
pub use stream::{AgentFeedback, ProgressSpinner, StreamManager};
use tree::build_root;

/// Name of the completion menu registered with the line editor.
const COMPLETION_MENU: &str = "kvist_completion";

/// Truncates text to a maximum character count, adding an ellipsis if needed.
fn truncate(text: &str, max_chars: usize) -> String {
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

/// Handles one trimmed shell line, never returning an error: every failure
/// mode is reported to the user and the session continues.
pub(crate) fn handle_line(
    line: &str,
    project_dir: &Path,
    journal: &SessionJournal,
    stream_manager: &StreamManager,
) -> LoopAction {
    let line = line.trim();
    if line.is_empty() {
        return LoopAction::Continue;
    }
    if line == "exit" || line == "quit" {
        return LoopAction::Exit;
    }

    // Session inspection builtins.
    if line == "journal" || line == "history" {
        display_session_status(journal);
        return LoopAction::Continue;
    }

    // Task-lock inspection and cleanup builtins.
    if line == "locks" || line == "locks clean" {
        handle_locks(line == "locks clean", journal);
        return LoopAction::Continue;
    }

    // In the interactive shell REPL, bare 'status' and 'overview' display the
    // human-friendly project overview.
    if line == "status" || line == "overview" {
        let result = (|| {
            let inspection = crate::project_state::inspect(project_dir)?;
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
                journal.append(journal_entry(line, &truncate(&text, 100)));
            }
            Err(error) => {
                let _ = error.print();
                journal.append(journal_entry(line, "error"));
            }
        }
        return LoopAction::Continue;
    }

    let (program, arguments) = match split_raw_command(line) {
        Ok(split) => split,
        Err(error) => {
            eprintln!("error: {error}");
            journal.append(journal_entry(line, "syntax error"));
            return LoopAction::Continue;
        }
    };

    // `prompt` is an explicit, high-signal authoring action and never falls
    // through to the CLI parser (an unknown task must not run as prompt text).
    if program == "prompt" {
        return match arguments.len() {
            0 => {
                println!(
                    "usage: prompt <TASK_ID> — opens your editor seeded with the task context"
                );
                journal.append(journal_entry(line, "usage hint"));
                LoopAction::Continue
            }
            1 => handle_prompt_authoring(line, project_dir, &arguments[0], journal, stream_manager),
            _ => {
                dispatch(line, journal, stream_manager);
                LoopAction::Continue
            }
        };
    }

    dispatch(line, journal, stream_manager);
    LoopAction::Continue
}

/// Lists live and stale task locks; `locks clean` removes the stale ones.
fn handle_locks(clean: bool, journal: &SessionJournal) -> LoopAction {
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
    display_output(&render_locks(&entries, &cleaned, clean));
    journal.append(journal_entry(
        command,
        &format!("{} locks, {} cleaned", entries.len(), cleaned.len()),
    ));
    LoopAction::Continue
}

/// Renders the task-lock listing (pure, so it is testable without user state).
fn render_locks(entries: &[locks::LockEntry], cleaned: &[PathBuf], clean: bool) -> String {
    let mut text = format!("Task locks ({}):\n", entries.len());
    for entry in entries {
        let state = if entry.live { "live" } else { "stale" };
        let task = entry.task_id.as_deref().unwrap_or("<unknown>");
        let pid = entry
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "<unknown>".to_owned());
        let age = locks::format_age(entry.age_secs);
        text.push_str(&format!(
            "  [{state}] task {task} · pid {pid} · age {age}\n"
        ));
    }
    for path in cleaned {
        text.push_str(&format!("  cleaned: {}\n", path.display()));
    }
    if clean && cleaned.is_empty() {
        text.push_str("  no stale locks to clean\n");
    }
    text
}

/// Runs the `prompt <task_id>` editor flow without ever killing the session.
fn handle_prompt_authoring(
    line: &str,
    project_dir: &Path,
    task_id: &str,
    journal: &SessionJournal,
    stream_manager: &StreamManager,
) -> LoopAction {
    let command = match prompt_editor_command(project_dir, task_id) {
        Ok(Some(command)) => command,
        Ok(None) => {
            eprintln!(
                "No such task `{task_id}`. Try `tasks` to list tasks or `overview` for project status."
            );
            journal.append(journal_entry(line, "no such task"));
            return LoopAction::Continue;
        }
        Err(error) => {
            let _ = error.print();
            journal.append(journal_entry(line, "editor error"));
            return LoopAction::Continue;
        }
    };

    let spinner = stream_manager.start_spinner("authoring prompt");
    stream_manager.print_prompt_stage(line);
    stream_manager.print_working_stage();
    let result = cli::execute(command, false);
    spinner.stop();
    stream_manager.print_result_stage(&result);

    let summary = match &result {
        Ok(out) => truncate(&out.to_string(), 100),
        Err(err) => truncate(&err.to_string(), 100),
    };
    journal.append(journal_entry(line, &summary));
    LoopAction::Continue
}

/// Dispatches a non-builtin command line, handling streaming, paging, and
/// journaling. Total: every failure is reported and the session continues.
fn dispatch(line: &str, journal: &SessionJournal, stream_manager: &StreamManager) {
    let (program, arguments) = match split_raw_command(line) {
        Ok(split) => split,
        Err(error) => {
            eprintln!("error: {error}");
            journal.append(journal_entry(line, "syntax error"));
            return;
        }
    };

    let mut full_args: Vec<String> = vec!["kvist".to_owned(), program.clone()];
    full_args.extend(arguments.iter().cloned());

    let parsed = match cli::Cli::try_parse_from(&full_args) {
        Ok(parsed) => parsed,
        Err(error) => {
            let _ = error.print();
            journal.append(journal_entry(line, "syntax error"));
            return;
        }
    };

    if matches!(parsed.command, cli::Command::Shell(_)) {
        println!("You are already in an active Kvist shell.");
        journal.append(journal_entry(line, "already in shell"));
        return;
    }

    let streaming = is_streaming_command(&parsed.command);
    let mut cmd = parsed.command;
    if streaming {
        if let cli::Command::Task {
            command: cli::TaskCommand::Run { ref mut stream, .. },
        } = cmd
        {
            *stream = true;
        }
        stream_manager.print_prompt_stage(line);
        stream_manager.print_working_stage();
    }

    // A spinner with elapsed time covers any command that outlasts its
    // deferred start, so slow work is visibly progressing rather than hung.
    let spinner = stream_manager.start_spinner(&program);
    let result = cli::execute(cmd, false);
    spinner.stop();

    if streaming {
        stream_manager.print_result_stage(&result);
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
    journal.append(journal_entry(line, &summary));
}

/// Returns whether a command type produces long-running output that should be streamed.
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

/// Builds the line editor with the Kvist completer, inline hinter, and IDE completion menu.
fn build_editor(completer: Box<KvistCompleter>) -> std::result::Result<Reedline, KvistError> {
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

    let edit_mode = Box::new(Emacs::new(keybindings));

    let editor = Reedline::create()
        .with_completer(completer)
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

    let root = build_root();
    let state = Arc::new(Mutex::new(DynamicState::load(project_dir)));
    let completer = Box::new(KvistCompleter::new(root, state.clone()));
    let status = StatusContext::load(project_dir);
    let journal = SessionJournal::new(project_dir);
    let stream_manager = StreamManager::new(project_dir);

    let mut editor = build_editor(completer)?;

    let initial_branch = state
        .lock()
        .ok()
        .and_then(|guard| guard.branch().map(str::to_owned));
    print_welcome_banner(&status, initial_branch.as_deref());

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
            left_prompt: DefaultPromptSegment::Basic(short_prompt(branch.as_deref())),
            right_prompt: DefaultPromptSegment::Basic(status_bar_label(&current_status)),
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
                if handle_line(&line, project_dir, &journal, &stream_manager) == LoopAction::Exit {
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
        }
    }

    // The journal is append-only on disk; nothing is buffered in memory.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn truncate_helper() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(
            truncate("a very long line of text exceeding limit", 10),
            "a very ..."
        );
    }

    #[test]
    fn is_streaming_command_classifies_correctly() {
        let task_run = cli::Command::Task {
            command: cli::TaskCommand::Run {
                component_dir: ".".into(),
                task_id: "t1".into(),
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
        let journal = SessionJournal::new(dir.path());
        let stream_manager = StreamManager::new(dir.path());
        dispatch("unknown-command", &journal, &stream_manager);
        assert_eq!(journal.entries().len(), 1);
        assert_eq!(journal.entries()[0].result, "syntax error");
    }

    #[test]
    fn dispatch_handles_shell_recursion() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        let stream_manager = StreamManager::new(dir.path());
        dispatch("shell", &journal, &stream_manager);
        assert_eq!(journal.entries()[0].result, "already in shell");
    }

    #[test]
    fn handle_line_exits_on_exit_and_quit() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        let stream_manager = StreamManager::new(dir.path());
        assert_eq!(
            handle_line("exit", dir.path(), &journal, &stream_manager),
            LoopAction::Exit
        );
        assert_eq!(
            handle_line("quit", dir.path(), &journal, &stream_manager),
            LoopAction::Exit
        );
    }

    #[test]
    fn handle_line_continues_on_empty_lines() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        let stream_manager = StreamManager::new(dir.path());
        assert_eq!(
            handle_line("", dir.path(), &journal, &stream_manager),
            LoopAction::Continue
        );
        assert_eq!(
            handle_line("   ", dir.path(), &journal, &stream_manager),
            LoopAction::Continue
        );
    }

    #[test]
    fn handle_line_routes_journal_builtin() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        let stream_manager = StreamManager::new(dir.path());
        assert_eq!(
            handle_line("journal", dir.path(), &journal, &stream_manager),
            LoopAction::Continue
        );
        assert_eq!(
            handle_line("history", dir.path(), &journal, &stream_manager),
            LoopAction::Continue
        );
    }

    #[test]
    fn handle_line_rejects_unknown_prompt_task_instead_of_falling_through() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        let stream_manager = StreamManager::new(dir.path());
        assert_eq!(
            handle_line("prompt no-such-task", dir.path(), &journal, &stream_manager),
            LoopAction::Continue
        );
        assert_eq!(journal.entries().len(), 1);
        assert_eq!(journal.entries()[0].result, "no such task");
    }

    #[test]
    fn handle_line_shows_prompt_usage_when_no_task_given() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        let stream_manager = StreamManager::new(dir.path());
        assert_eq!(
            handle_line("prompt", dir.path(), &journal, &stream_manager),
            LoopAction::Continue
        );
        assert_eq!(journal.entries()[0].result, "usage hint");
    }

    #[test]
    fn handle_line_never_propagates_unparseable_lines() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        let stream_manager = StreamManager::new(dir.path());
        assert_eq!(
            handle_line("task", dir.path(), &journal, &stream_manager),
            LoopAction::Continue
        );
        assert_eq!(journal.entries().len(), 1);
        assert_eq!(journal.entries()[0].result, "syntax error");
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
        let text = render_locks(&entries, &[], false);
        assert!(text.starts_with("Task locks (2):\n"));
        assert!(text.contains("[live] task write-tests · pid 4242 · age 1m"));
        assert!(text.contains("[stale] task <unknown>"));

        let cleaned = vec![PathBuf::from("/state/kvist/task-locks-v1/x.lock")];
        let text = render_locks(&entries, &cleaned, true);
        assert!(text.contains("cleaned: /state/kvist/task-locks-v1/x.lock"));

        let text = render_locks(&entries, &[], true);
        assert!(text.contains("no stale locks to clean"));
    }
}
