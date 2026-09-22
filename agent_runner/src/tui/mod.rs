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
use crate::retry::RetryPolicy;
use crate::run::{self, start};
use crate::session::{AgentSession, MAX_TURNS, Recorder};
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
    /// Model context window in tokens. `None` uses the default (8192).
    pub context_limit: Option<usize>,
    /// Disable durable session logging entirely.
    pub no_logs: bool,
    /// The resolved configuration path, shown in the help overlay.
    pub config_path: Option<PathBuf>,
    /// When true, each prompt may drive an autonomous multi-turn loop rather
    /// than a single model turn. A single turn is the safe default.
    pub multi_turn: bool,
    /// A prefilled prompt that is auto-started when the session begins.
    pub prompt: Option<String>,
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

    let effort = overrides.effort.unwrap_or(config.default_thinking_effort);
    let working_directory = overrides
        .cwd
        .clone()
        .unwrap_or_else(|| config.working_directory.clone());

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
    let registry = match ToolRegistry::resolve(
        config.tool_policy.clone(),
        &config.tool_profiles,
        &probe,
        forced,
    ) {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("{}", error.describe());
            return ExitCode::from(error.exit_code());
        }
    };
    let tool_defs = registry.tool_definitions();

    let executor = Arc::new(SandboxExecutor::new(
        registry,
        config.sandbox.clone(),
        working_directory.clone(),
    ));

    let builder = SessionBuilder {
        config: config.clone(),
        executor,
        tool_defs,
        context_limit: overrides
            .context_limit
            .unwrap_or(crate::context::DEFAULT_CONTEXT_TOKENS),
        log_dir: overrides.log_dir.clone(),
        no_logs: overrides.no_logs,
        working_directory,
        max_turns: if overrides.multi_turn { MAX_TURNS } else { 1 },
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
    config: Config,
    executor: Arc<SandboxExecutor>,
    tool_defs: Vec<agent_runtime::ToolDefinition>,
    context_limit: usize,
    log_dir: Option<PathBuf>,
    no_logs: bool,
    working_directory: PathBuf,
    max_turns: u32,
}

/// One live session worker: the model loop thread plus its channels.
struct Worker {
    handle: run::SessionHandle,
    rx: mpsc::Receiver<crate::session::Event>,
    prompt_tx: mpsc::Sender<String>,
    model_id: String,
    effort: ReasoningEffort,
}

impl SessionBuilder {
    /// Starts a session worker for `model` with `effort`.
    fn start(&self, model: &Model, effort: ReasoningEffort) -> Result<Worker> {
        let transport = build_transport(model)?;
        // Derive the retry policy from the model's own settings so a turn that
        // is generation-bound recovers from transient failures with back-off.
        let retry = RetryPolicy::new(
            model.max_attempts,
            Duration::from_secs(model.retry_base_delay_secs),
            Duration::from_secs(model.retry_max_delay_secs),
        );
        let session = AgentSession::new(
            model.clone(),
            effort,
            self.tool_defs.clone(),
            run::system_prompt(&self.config.tool_policy.write_root),
        );
        let context = ContextManager::new(self.context_limit, 6);
        let recorder = if self.no_logs {
            None
        } else {
            let dir = self.log_dir.clone().unwrap_or_else(|| {
                self.working_directory
                    .join(crate::session_log::DEFAULT_LOG_DIR)
            });
            match SessionLog::open(&dir, format!("agent-runner-{}", model.id)) {
                Ok(log) => Some(Box::new(log) as Box<dyn Recorder>),
                Err(error) => {
                    // Logging is best-effort: report, then continue without it.
                    eprintln!(
                        "{}",
                        Error::Io {
                            operation: "open session log directory".to_owned(),
                            path: Some(dir.to_string_lossy().into_owned()),
                            source: error,
                        }
                        .describe()
                    );
                    None
                }
            }
        };
        let (handle, rx, prompt_tx) = start(
            session,
            transport,
            Arc::clone(&self.executor),
            context,
            recorder,
            retry,
            self.max_turns,
        );
        Ok(Worker {
            handle,
            rx,
            prompt_tx,
            model_id: model.id.clone(),
            effort,
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

fn build_transport(model: &Model) -> Result<agent_runtime::DirectModelTransport> {
    let provider = model.provider.to_agent_provider();
    let deadline = Duration::from_secs(model.deadline_secs.max(1));
    let mut transport = agent_runtime::DirectModelTransport::new(
        provider,
        &model.base_url,
        deadline,
        8 * 1024 * 1024,
    )
    .map_err(|error| Error::ModelTransport {
        model: Some(model.id.clone()),
        reason: error.to_string(),
    })?;
    // A generous per-turn deadline is safe to relax because the inter-token
    // cadence watchdog bounds a stalled provider: if no token arrives within
    // `cadence_timeout_secs` after the first token, the turn is retried instead
    // of hanging until the (long) deadline. `0` disables the watchdog.
    if model.cadence_timeout_secs > 0 {
        transport = transport.with_cadence_timeout(Duration::from_secs(model.cadence_timeout_secs));
    }
    Ok(transport)
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
                                *worker = Some(builder.start(model, app.effort)?);
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
            break;
        }
    }
    Ok(())
}
