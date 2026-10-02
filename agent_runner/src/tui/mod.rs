//! The terminal UI: setup, event loop, and teardown.

mod app;
mod render;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use agent_runtime::ReasoningEffort;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, KeyEventKind,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, size,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use self::app::{App, KeyAction};
use crate::config::{Config, Model};
use crate::context::ContextManager;
use crate::error::{Error, Result};
use crate::executor::SandboxExecutor;
use crate::host::HostExecutor;
use crate::run::{self, start};
use crate::session::{AgentSession, MAX_TURNS, Recorder, ToolExecutor};
use crate::session_log::SessionLog;
use crate::tools::ToolRegistry;

/// User-selected overrides applied over a loaded configuration.
pub struct Overrides {
    /// The model id to select for this session.
    pub model: Option<String>,
    /// The thinking effort to select for this session.
    pub effort: Option<ReasoningEffort>,
    /// The working directory to select for this session.
    pub cwd: Option<PathBuf>,
    /// The language tool profile to select for this session.
    pub profile: Option<crate::tools::ToolProfile>,
    /// Directory for the session journal + transcript. `None` means the
    /// default (`.agent-runner/runs` under the working directory).
    pub log_dir: Option<PathBuf>,
    /// Serving context override. `None` uses model configuration or discovery.
    pub context_limit: Option<usize>,
    /// Generation reserve override; absence uses model configuration/default.
    pub response_reserve: Option<u32>,
    /// Disable durable session logging entirely.
    pub no_logs: bool,
    /// The resolved configuration path, shown in the help overlay.
    pub config_path: Option<PathBuf>,
    /// When true, the agent runs its commands on the host (no sandbox). Off by
    /// default: the agent is confined to the sandbox, multi-turn, and needs no
    /// acknowledgement.
    pub allow_host_execution: bool,
    /// The maximum autonomous turns a prompt drives when `allow_host_execution`
    /// is set. `None` means the safe default (one turn).
    pub host_turns: Option<u32>,
    /// A prefilled prompt that is auto-started when the session begins.
    pub prompt: Option<String>,
    /// Shared prompt and provider response limits.
    pub limits: crate::session::RunLimits,
}

/// Resolves the autonomous turn cap for a session and validates it.
///
/// Sandboxed execution is the safe default and is multi-turn (`MAX_TURNS`). Host
/// execution runs with real privileges, so it is single-turn by default and only
/// `--host-turns` lifts the cap — and only within `1..=MAX_TURNS`, because an
/// out-of-range cap would otherwise be a silent no-op. Returns the resolved cap,
/// or [`Error::HostTurns`] when a host cap was requested outside that range.
fn resolve_max_turns(allow_host_execution: bool, host_turns: Option<u32>) -> Result<u32> {
    if allow_host_execution {
        match host_turns {
            Some(turns) if (1..=MAX_TURNS).contains(&turns) => Ok(turns),
            Some(turns) => Err(Error::HostTurns {
                requested: turns,
                max: MAX_TURNS,
            }),
            None => Ok(1),
        }
    } else if host_turns.is_some() {
        Err(Error::Config {
            path: None,
            reason: "host-turn overrides require explicit host execution".into(),
        })
    } else {
        Ok(MAX_TURNS)
    }
}

/// Runs the interactive UI to completion and returns the process exit code.
pub fn run(config: Config, overrides: Overrides) -> ExitCode {
    if !std::io::stdin().is_terminal() {
        eprintln!(
            "{}",
            Error::NotInteractive {
                reason: "standard input is not a terminal".to_owned()
            }
            .describe()
        );
        return ExitCode::from(2);
    }

    let model_id = overrides.model.as_deref().unwrap_or(&config.default_model);
    let model = match config.model(model_id) {
        Some(model) => model.clone(),
        None => {
            let available = config
                .models
                .iter()
                .map(|model| model.id.clone())
                .collect::<Vec<_>>();
            eprintln!(
                "{}",
                Error::ModelNotFound {
                    requested: model_id.to_owned(),
                    available,
                }
                .describe()
            );
            return ExitCode::from(2);
        }
    };

    let app_model_label = app_model_id(&config, &model);
    if let Err(error) = overrides.limits.validate() {
        eprintln!("{}", error.describe());
        return ExitCode::from(error.exit_code());
    }

    let effort = overrides.effort.unwrap_or(config.default_thinking_effort);
    let working_directory = match crate::config::resolve_working_directory(
        overrides
            .cwd
            .as_deref()
            .unwrap_or(&config.working_directory),
    ) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("{}", error.describe());
            return ExitCode::from(error.exit_code());
        }
    };

    // Resolve the session-transcript directory once so both the worker and the
    // history overlay read and write the same location.
    let log_dir = overrides.log_dir.clone().unwrap_or_else(|| {
        working_directory
            .clone()
            .join(crate::session_log::DEFAULT_LOG_DIR)
    });

    // Advertise only tool-chains that genuinely reach the sandbox, gated on the
    // per-language profile settings. Detection is advisory logging; the gate is
    // the sole authority for what is advertised. An explicit/forced profile or a
    // profile set to `on` that is missing fails here rather than lying.
    let probe = crate::toolchain::HostProbe;
    let forced = overrides.profile;
    let entry_names: Vec<String> = std::fs::read_dir(&working_directory)
        .map(|dir| {
            dir.flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let names: Vec<&str> = entry_names.iter().map(String::as_str).collect();
    let detected = crate::toolchain::detect_languages(&names);
    if !detected.is_empty() {
        eprintln!(
            "detected project language(s): {}",
            detected
                .iter()
                .map(|profile| profile.id())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let resolved = if overrides.allow_host_execution {
        ToolRegistry::resolve(
            config.tool_policy.clone(),
            &config.tool_profiles,
            &probe,
            forced,
        )
    } else {
        ToolRegistry::resolve_for_workspace(
            config.tool_policy.clone(),
            &config.tool_profiles,
            &probe,
            forced,
            &working_directory,
        )
    };
    let registry = match resolved {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("{}", error.describe());
            return ExitCode::from(error.exit_code());
        }
    };
    let resource_notes = registry.diagnostics().to_vec();
    let tool_defs = registry.tool_definitions();

    // Resolve the autonomous turn cap for this session, validating the host cap.
    let max_turns = match resolve_max_turns(overrides.allow_host_execution, overrides.host_turns) {
        Ok(turns) => turns,
        Err(error) => {
            eprintln!("{}", error.describe());
            return ExitCode::from(error.exit_code());
        }
    };

    let executor: Arc<dyn ToolExecutor> = if overrides.allow_host_execution {
        eprintln!(
            "warning: --allow-host-execution runs the agent on the host with real \
             privileges; a single prompt is single-turn by default (raise --host-turns \
             to allow more)"
        );
        Arc::new(HostExecutor::new(registry, working_directory.clone()))
    } else {
        Arc::new(SandboxExecutor::new(
            registry,
            config.sandbox.clone(),
            working_directory.clone(),
        ))
    };

    let builder = SessionBuilder {
        config: config.clone(),
        executor,
        tool_defs,
        context_limit: overrides.context_limit,
        response_reserve: overrides.response_reserve,
        log_dir: overrides.log_dir.clone(),
        no_logs: overrides.no_logs,
        working_directory,
        max_turns,
        limits: overrides.limits,
        resource_notes,
        allow_host_execution: overrides.allow_host_execution,
    };

    let initial = match builder.start(&model, effort) {
        Ok(worker) => worker,
        Err(error) => {
            eprintln!("{}", error.describe());
            return ExitCode::from(error.exit_code());
        }
    };

    let (width, height) = size().unwrap_or((100, 30));
    let model_ids: Vec<String> = config.models.iter().map(|model| model.id.clone()).collect();
    let mut app = App::new(
        &model_ids,
        &app_model_label,
        effort,
        overrides.config_path,
        width,
        height,
    );
    app.push_event(crate::session::Event::Note(initial.budget_note.clone()));
    for note in &builder.resource_notes {
        app.push_event(crate::session::Event::Note(note.clone()));
    }
    // Point the history overlay at the same directory the worker logs to, so
    // "Session history" lists the transcripts this run contributes to.
    app.set_log_dir(log_dir);
    app.execution_scope = if overrides.allow_host_execution {
        crate::session_log::ExecutionScope::HostUnconfined
    } else {
        crate::session_log::ExecutionScope::SandboxedWorkspace
    };
    // Prefill and auto-start a caller-supplied prompt (for example one produced
    // by `kvist prompt`). The startup dispatch in `run_ui` sends the staged
    // text to the worker before the interactive loop begins.
    if let Some(prompt) = overrides.prompt.clone() {
        app.stage_initial_prompt(prompt);
    }

    match ui_loop(app, &builder, Some(initial)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.describe());
            ExitCode::from(error.exit_code())
        }
    }
}

/// Builds session workers for the selected model and effort. Each worker owns
/// its transport, conversation, context window, and durable log; switching the
/// model or the effort starts a fresh worker (the model's conversation context
/// restarts, which the UI announces).
struct SessionBuilder {
    resource_notes: Vec<String>,
    config: Config,
    executor: Arc<dyn ToolExecutor>,
    tool_defs: Vec<agent_runtime::ToolDefinition>,
    context_limit: Option<usize>,
    response_reserve: Option<u32>,
    log_dir: Option<PathBuf>,
    no_logs: bool,
    working_directory: PathBuf,
    max_turns: u32,
    limits: crate::session::RunLimits,
    allow_host_execution: bool,
}

/// One live session worker: the model loop thread plus its channels.
struct Worker {
    // Close both channels before the joining handle is dropped.
    prompt_tx: mpsc::Sender<String>,
    rx: mpsc::Receiver<crate::session::Event>,
    handle: run::SessionHandle,
    model_id: String,
    effort: ReasoningEffort,
    budget_note: String,
}

impl SessionBuilder {
    /// Starts a session worker for `model` with `effort`.
    fn start(&self, model: &Model, effort: ReasoningEffort) -> Result<Worker> {
        let budgets =
            model.resolve_budgets(self.context_limit, self.response_reserve, self.limits)?;
        let transport = model.transport()?;
        // Derive the retry policy from the model's own settings so a turn that
        // is generation-bound recovers from transient failures with back-off.
        let retry = model.retry_policy();
        let session = AgentSession::new(
            model.clone(),
            effort,
            self.tool_defs.clone(),
            if self.allow_host_execution {
                run::host_system_prompt(
                    &self.config.tool_policy.write_root,
                    &self.working_directory,
                )
            } else {
                run::system_prompt(&self.config.tool_policy.write_root)
            },
        );
        let context = ContextManager::for_model(budgets.context_limit);
        let mut recorder = if self.no_logs {
            None
        } else {
            let dir = self.log_dir.clone().unwrap_or_else(|| {
                self.working_directory
                    .join(crate::session_log::DEFAULT_LOG_DIR)
            });
            Some(Box::new(
                SessionLog::open(&dir, format!("agent-runner-{}", model.id))
                    .map_err(|source| {
                        crate::error::io_error(
                            "open private session log directory",
                            Some(&dir.to_string_lossy()),
                            source,
                        )
                    })?
                    .with_metadata(crate::session_log::SessionMetadata {
                        execution_scope: if self.allow_host_execution {
                            crate::session_log::ExecutionScope::HostUnconfined
                        } else {
                            crate::session_log::ExecutionScope::SandboxedWorkspace
                        },
                        working_directory: self.working_directory.clone(),
                        write_root: self.config.tool_policy.write_root.clone(),
                        policy_identity: self.config.tool_policy.identity(),
                        context_limit: budgets.context_limit,
                        context_source: budgets.context_source.into(),
                        response_reserve: budgets.limits.response_reserve,
                        response_source: budgets.response_source.into(),
                        max_run_tokens: self.limits.max_tokens,
                        max_run_secs: self.limits.wall_time.as_secs(),
                    }),
            ) as Box<dyn Recorder>)
        };
        if let Some(log) = recorder.as_mut() {
            for note in &self.resource_notes {
                log.notice(note)?;
            }
        }
        let (handle, rx, prompt_tx) = start(
            session,
            transport,
            Arc::clone(&self.executor),
            context,
            recorder,
            retry,
            self.max_turns,
            budgets.limits,
        )?;
        Ok(Worker {
            handle,
            rx,
            prompt_tx,
            model_id: model.id.clone(),
            effort,
            budget_note: format!(
                "model budget: {} context tokens ({}), {} output tokens ({}); context/usage accounting is estimated unless provider usage is available",
                budgets.context_limit,
                budgets.context_source,
                budgets.limits.response_reserve,
                budgets.response_source,
            ),
        })
    }
}

fn app_model_id(config: &Config, model: &Model) -> String {
    config
        .models
        .iter()
        .find(|candidate| candidate.id == model.id)
        .map(|candidate| candidate.id.clone())
        .unwrap_or_else(|| model.id.clone())
}

fn ui_loop(mut app: App, builder: &SessionBuilder, mut worker: Option<Worker>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_ui(&mut terminal, &mut app, builder, &mut worker);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

fn run_ui<M: std::io::Write>(
    terminal: &mut Terminal<CrosstermBackend<M>>,
    app: &mut App,
    builder: &SessionBuilder,
    worker: &mut Option<Worker>,
) -> Result<()> {
    // Dispatch any prefilled, auto-started prompt (for example one supplied by
    // `kvist prompt`) before the interactive loop, and clear the input so the
    // same text cannot be submitted again.
    if let Some(text) = app.take_pending_prompt() {
        app.editor.clear();
        if let Some(current) = worker.as_ref() {
            let _ = current.prompt_tx.send(text);
        }
    }

    loop {
        terminal.draw(|frame| render::render(frame, app))?;

        // Emit any staged OSC 52 clipboard escape before drawing, so the write
        // goes through this loop's stdout handle and the buffer is flushed each
        // iteration. Copying is a terminal-side convenience, so a failure here is
        // reported but never aborts the run.
        if let Err(error) = crate::tui::app::emit_pending_osc_52(app) {
            eprintln!("copy: {error}");
        }

        // Drain loop events without blocking.
        if let Some(current) = worker.as_ref() {
            app.pump(&current.rx)?;
        }

        if event::poll(Duration::from_millis(150))? {
            match event::read()? {
                CrosstermEvent::Resize(width, height) => app.resize(width, height),
                CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                    match app.on_key(key) {
                        KeyAction::Submit => {
                            let text = app.take_pending_prompt();
                            let changed = match worker.as_ref() {
                                Some(current) => {
                                    current.model_id != app.model || current.effort != app.effort
                                }
                                None => true,
                            };
                            if text.is_some() && changed {
                                // A model or effort change applies to the next
                                // prompt: the old worker is cancelled and
                                // joined (dropped), then a fresh session starts.
                                let model = builder.config.model(&app.model).ok_or_else(|| {
                                    Error::ModelNotFound {
                                        requested: app.model.clone(),
                                        available: builder
                                            .config
                                            .models
                                            .iter()
                                            .map(|model| model.id.clone())
                                            .collect(),
                                    }
                                })?;
                                let replacement = builder.start(model, app.effort)?;
                                app.push_event(crate::session::Event::Note(
                                    replacement.budget_note.clone(),
                                ));
                                *worker = Some(replacement);
                            }
                            if let (Some(text), Some(current)) = (text, worker.as_ref()) {
                                let _ = current.prompt_tx.send(text);
                            }
                        }
                        KeyAction::Cancel => {
                            if let Some(current) = worker.as_ref() {
                                current.handle.cancel();
                            }
                        }
                        KeyAction::Quit => {
                            app.should_quit = true;
                        }
                        KeyAction::Idle => {}
                    }
                }
                CrosstermEvent::Key(_) => {}
                CrosstermEvent::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollUp => {
                    app.scroll_up(3);
                }
                CrosstermEvent::Mouse(mouse) if mouse.kind == MouseEventKind::ScrollDown => {
                    app.scroll_down(3);
                }
                _ => {}
            }
        }

        if app.should_quit {
            // Graceful teardown before the loop exits. The worker blocks on
            // rx.recv() waiting for the next prompt, so it can only exit once
            // every prompt sender is dropped. Dropping the session handle first
            // would join() a thread that can never unblock: the only prompt
            // sender lives in the worker we are dropping. That ordering made
            // the TUI hang after it closed and forced Ctrl-C. So cancel the
            // running turn, then drop the sender to unblock recv(), and let the
            // handle (which joins) drop last.
            if let Some(worker) = worker.take() {
                worker.handle.cancel();
                drop(worker.prompt_tx);
            }
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_working_directory_rejects_missing_and_regular_paths() {
        let dir = tempfile::tempdir().expect("directory");
        let regular = dir.path().join("file");
        std::fs::write(&regular, b"not a directory").expect("file");
        for path in [dir.path().join("missing"), regular] {
            assert!(crate::config::resolve_working_directory(&path).is_err());
        }
    }

    #[test]
    fn startup_working_directory_canonicalizes_relative_and_link_paths() {
        let current = std::env::current_dir().expect("current directory");
        assert_eq!(
            crate::config::resolve_working_directory(std::path::Path::new(".")).expect("relative"),
            current.canonicalize().expect("canonical")
        );
        let dir = tempfile::tempdir().expect("directory");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&current, &link).expect("link");
        assert_eq!(
            crate::config::resolve_working_directory(&link).expect("linked directory"),
            current.canonicalize().expect("canonical")
        );
    }

    #[test]
    fn sandboxed_work_is_multi_turn_by_default() {
        assert_eq!(
            resolve_max_turns(false, None).expect("default cap"),
            MAX_TURNS,
            "a sandboxed prompt runs multi-turn without any flag"
        );
    }

    #[test]
    fn sandboxed_rejects_any_host_turn_cap() {
        for turns in [0, 1, MAX_TURNS, u32::MAX] {
            assert!(matches!(
                resolve_max_turns(false, Some(turns)),
                Err(Error::Config { .. })
            ));
        }
    }

    #[test]
    fn host_execution_is_single_turn_by_default() {
        assert_eq!(
            resolve_max_turns(true, None).expect("default cap"),
            1,
            "host work is single-turn by default to bound privilege overuse"
        );
    }

    #[test]
    fn host_turns_lifts_the_cap_when_in_range() {
        assert_eq!(
            resolve_max_turns(true, Some(10)).expect("cap"),
            10,
            "an in-range host cap is honored"
        );
        assert_eq!(
            resolve_max_turns(true, Some(MAX_TURNS)).expect("cap"),
            MAX_TURNS,
            "the maximum in-range value is honored"
        );
    }

    #[test]
    fn host_turns_without_host_execution_is_rejected_by_resolver() {
        assert!(resolve_max_turns(false, Some(5)).is_err());
    }

    #[test]
    fn host_turns_outside_the_range_is_rejected() {
        assert!(
            resolve_max_turns(true, Some(0)).is_err(),
            "zero is below range"
        );
        assert!(
            resolve_max_turns(true, Some(MAX_TURNS + 1)).is_err(),
            "above range is rejected"
        );
    }
}
