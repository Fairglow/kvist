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
mod pager;
mod prompt_editor;
mod state;
mod status;
mod stream;
mod tree;

use std::path::Path;
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
pub use status::{
    ActiveLocks, LockInfo, StatusContext, print_welcome_banner, prompt_label, short_prompt,
    status_bar_label,
};
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

/// Dispatches a single command line, handling streaming, paging, and journaling.
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
            stream_manager.print_prompt_stage(line);
            stream_manager.print_working_stage();
            let result = cli::execute(command, false);
            stream_manager.print_result_stage(&result);
            let summary = match &result {
                Ok(out) => truncate(&out.to_string(), 100),
                Err(err) => truncate(&err.to_string(), 100),
            };
            journal.append(JournalEntry {
                timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                command: format!("prompt {}", arguments[0]),
                result: summary,
                transient: false,
            });
            return Ok(());
        }
    }

    let mut full_args: Vec<String> = vec!["kvist".to_owned(), program.clone()];
    full_args.extend(arguments.iter().cloned());

    match cli::Cli::try_parse_from(&full_args) {
        Ok(parsed) => {
            if matches!(parsed.command, cli::Command::Shell(_)) {
                println!("You are already in an active Kvist shell.");
                return Ok(());
            }

            let should_stream = is_streaming_command(&parsed.command);
            if should_stream {
                let mut cmd = parsed.command;
                if let cli::Command::Task {
                    command: cli::TaskCommand::Run { ref mut stream, .. },
                } = cmd
                {
                    *stream = true;
                }

                stream_manager.print_prompt_stage(line);
                stream_manager.print_working_stage();

                let result = cli::execute(cmd, false);
                stream_manager.print_result_stage(&result);

                let summary = match &result {
                    Ok(out) => truncate(&out.to_string(), 100),
                    Err(err) => truncate(&err.to_string(), 100),
                };
                journal.append(JournalEntry {
                    timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    command: line.to_string(),
                    result: summary,
                    transient: false,
                });
            } else {
                let result = cli::execute(parsed.command, false);
                let summary = match result {
                    Ok(output) => {
                        let text = output.to_string();
                        display_output(&text);
                        truncate(&text, 100)
                    }
                    Err(error) => {
                        let msg = error.to_string();
                        let _ = error.print();
                        truncate(&msg, 100)
                    }
                };
                journal.append(JournalEntry {
                    timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    command: line.to_string(),
                    result: summary,
                    transient: false,
                });
            }

            Ok(())
        }
        Err(error) => {
            let _ = error.print();
            journal.append(JournalEntry {
                timestamp: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                command: line.to_string(),
                result: "syntax error".into(),
                transient: false,
            });
            Ok(())
        }
    }
}

/// Returns whether a command type produces long-running output that should be streamed.
fn is_streaming_command(command: &cli::Command) -> bool {
    match command {
        cli::Command::Task { command } => matches!(
            command,
            cli::TaskCommand::Run { .. } | cli::TaskCommand::Recover { .. }
        ),
        cli::Command::Agent { .. } => true,
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
        .with_quick_completions(false);
    Ok(editor)
}

/// Launches and runs the persistent interactive workspace shell (REPL).
pub fn run_shell(project_dir: &Path) -> Result<()> {
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

                // Handle journal / history inspection
                if line == "journal" || line == "history" {
                    display_session_status(&journal)?;
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
    }

    #[test]
    fn dispatch_handles_unknown_command_gracefully() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        let stream_manager = StreamManager::new(dir.path());
        assert!(dispatch("unknown-command", dir.path(), &journal, &stream_manager).is_ok());
        assert_eq!(journal.entries().len(), 1);
        assert_eq!(journal.entries()[0].result, "syntax error");
    }

    #[test]
    fn dispatch_handles_shell_recursion() {
        let dir = tempdir().unwrap();
        let journal = SessionJournal::new(dir.path());
        let stream_manager = StreamManager::new(dir.path());
        assert!(dispatch("shell", dir.path(), &journal, &stream_manager).is_ok());
    }
}
