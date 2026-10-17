//! The terminal UI: setup, event loop, and teardown.

pub mod app;
mod render;
mod startup;
pub mod theme;

pub use theme::file as theme_files;
pub use theme::{THEME_NAMES, Theme};

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, mpsc};
use std::time::Duration;

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
use sav::ReasoningEffort;

use self::app::{App, KeyAction};
use self::startup::Job;
use crate::config::{Config, Model};
use crate::context::ContextManager;
use crate::error::{Error, Result};
use crate::executor::SandboxExecutor;
use crate::host::HostExecutor;
use crate::run::{self, start};
use crate::session::{AgentSession, MAX_TURNS, Recorder, ToolExecutor};
use crate::session_transcript::SessionTranscript;
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
    /// default (`.skott/runs` under the working directory).
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
    /// The UI theme, resolved from the `--theme` flag over the configuration's
    /// `theme` key. `None` falls back to the configured (or default dark)
    /// theme.
    pub theme: Option<Theme>,
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

    if let Some(id) = overrides.model.as_deref()
        && config.model(id).is_none()
    {
        let available = config.models.iter().map(|model| model.id.clone()).collect();
        eprintln!(
            "{}",
            Error::ModelNotFound {
                requested: id.to_owned(),
                available
            }
            .describe()
        );
        return ExitCode::from(2);
    }
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

    // Resolve the autonomous turn cap for this session, validating the host cap.
    let max_turns = match resolve_max_turns(overrides.allow_host_execution, overrides.host_turns) {
        Ok(turns) => turns,
        Err(error) => {
            eprintln!("{}", error.describe());
            return ExitCode::from(error.exit_code());
        }
    };

    let (width, height) = size().unwrap_or((100, 30));
    let model_ids: Vec<String> = config.models.iter().map(|model| model.id.clone()).collect();
    let theme = overrides
        .theme
        .clone()
        .unwrap_or_else(|| config.theme.clone());
    let themes_dir = overrides
        .config_path
        .as_deref()
        .and_then(theme_files::themes_dir_for_config)
        .or_else(theme_files::bundled_themes_dir);
    let mut app = App::new(
        &model_ids,
        overrides.model.as_deref(),
        effort,
        overrides.config_path.clone(),
        width,
        height,
        theme,
    );
    app.set_theme_catalog(themes_dir);
    app.model_discovery_pending = overrides.model.is_none();
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

    if overrides.allow_host_execution {
        app.push_event(crate::session::Event::Note(
            "HOST UNCONFINED: tools run with host privileges".into(),
        ));
    }
    let discovery = match Discovery::start(config.clone(), overrides, working_directory, max_turns)
    {
        Ok(discovery) => discovery,
        Err(error) => {
            eprintln!("{}", error.describe());
            return ExitCode::from(error.exit_code());
        }
    };
    match ui_loop(app, &config, discovery) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.describe());
            ExitCode::from(error.exit_code())
        }
    }
}

/// Starts a session worker for `model` with `effort` on a background bootstrap
/// thread. Dropping its owner cancels discovery and joins the bootstrap.
fn spawn_bootstrap(
    builder: &SessionBuilder,
    model: &Model,
    effort: ReasoningEffort,
) -> Result<Bootstrap> {
    let builder = builder.clone();
    let model = model.clone();
    let model_id = model.id.clone();
    let job = Job::spawn("agent-bootstrap", move |token| {
        builder.start(&model, effort, token)
    })?;
    Ok(Bootstrap {
        job,
        model_id,
        effort,
    })
}

struct Bootstrap {
    job: Job<Worker>,
    model_id: String,
    effort: ReasoningEffort,
}

struct Discovery {
    tools: Option<Job<SessionBuilder>>,
    model: Option<Job<crate::config::Select>>,
}

impl Discovery {
    fn start(
        config: Config,
        overrides: Overrides,
        working_directory: PathBuf,
        max_turns: u32,
    ) -> Result<Self> {
        let model_config = config.clone();
        let explicit = overrides.model.clone();
        let model = Job::spawn("model-discovery", move |token| {
            let active = if explicit.is_some() {
                std::collections::BTreeMap::new()
            } else {
                crate::config::probe_active_models_cancellable(&model_config, token)
            };
            crate::executor::check_cancelled(token)?;
            Ok(crate::config::select_active_model(
                &model_config,
                explicit.as_deref(),
                &active,
            ))
        })?;
        let tools = Job::spawn("toolchain-discovery", move |token| {
            let detected = crate::toolchain::detect_project_languages(&working_directory);
            tracing::debug!(?detected, "project language discovery complete");
            crate::executor::check_cancelled(token)?;
            let registry = if overrides.allow_host_execution {
                ToolRegistry::resolve(
                    config.tool_policy.clone(),
                    &config.tool_profiles,
                    &crate::toolchain::HostProbe,
                    overrides.profile,
                )?
            } else {
                ToolRegistry::resolve_for_workspace_cancellable(
                    config.tool_policy.clone(),
                    &config.tool_profiles,
                    &crate::toolchain::HostProbe,
                    overrides.profile,
                    &working_directory,
                    token,
                )?
            };
            crate::executor::check_cancelled(token)?;
            let resource_notes = registry.diagnostics().to_vec();
            let mut summary_notes = vec![format!("Toolchains: {}", registry.profiles().join(", "))];
            if let Some(rust) = registry.rust_environment() {
                summary_notes.push(rust.short_diagnostic());
            } else if !resource_notes.is_empty() {
                summary_notes.push("Rust unavailable (auto; not advertised)".into());
            }
            let tool_defs = registry.tool_definitions();
            let executor: Arc<dyn ToolExecutor> = if overrides.allow_host_execution {
                Arc::new(HostExecutor::new(registry, working_directory.clone()))
            } else {
                Arc::new(SandboxExecutor::new(
                    registry,
                    config.sandbox.clone(),
                    working_directory.clone(),
                ))
            };
            Ok(SessionBuilder {
                resource_notes,
                summary_notes,
                config,
                executor,
                tool_defs,
                context_limit: overrides.context_limit,
                response_reserve: overrides.response_reserve,
                log_dir: overrides.log_dir,
                no_logs: overrides.no_logs,
                working_directory,
                max_turns,
                limits: overrides.limits,
                allow_host_execution: overrides.allow_host_execution,
            })
        })?;
        Ok(Self {
            tools: Some(tools),
            model: Some(model),
        })
    }

    fn cancel(&self) {
        if let Some(job) = &self.tools {
            job.cancel();
        }
        if let Some(job) = &self.model {
            job.cancel();
        }
    }
}

fn apply_selection(app: &mut App, selection: crate::config::Select) {
    app.model_discovery_pending = false;
    if app.model.is_some() {
        return;
    }
    let (id, state) = match selection {
        crate::config::Select::Explicit(id) => (Some(id), "selected"),
        crate::config::Select::Active { model_id, .. } => (Some(model_id), "already active"),
        crate::config::Select::Single { model_id } => (Some(model_id), "selected"),
        crate::config::Select::Default { model_id, .. } => (Some(model_id), "default"),
        crate::config::Select::NeedsSelection => (None, "press Tab to choose"),
    };
    let label = id.as_deref().unwrap_or("none");
    app.push_event(crate::session::Event::Note(format!(
        "Model: {label} ({state})"
    )));
    app.model = id;
}

/// Builds session workers for the selected model and effort. Each worker owns
/// its transport, conversation, context window, and durable log; switching the
/// model or the effort starts a fresh worker (the model's conversation context
/// restarts, which the UI announces). Cloned wholesale into the bootstrap
/// thread that starts the worker off the UI critical path.
struct SessionBuilder {
    resource_notes: Vec<String>,
    summary_notes: Vec<String>,
    config: Config,
    executor: Arc<dyn ToolExecutor>,
    tool_defs: Vec<sav::ToolDefinition>,
    context_limit: Option<usize>,
    response_reserve: Option<u32>,
    log_dir: Option<PathBuf>,
    no_logs: bool,
    working_directory: PathBuf,
    max_turns: u32,
    limits: crate::session::RunLimits,
    allow_host_execution: bool,
}

impl Clone for SessionBuilder {
    fn clone(&self) -> Self {
        Self {
            resource_notes: self.resource_notes.clone(),
            summary_notes: self.summary_notes.clone(),
            config: self.config.clone(),
            executor: Arc::clone(&self.executor),
            tool_defs: self.tool_defs.clone(),
            context_limit: self.context_limit,
            response_reserve: self.response_reserve,
            log_dir: self.log_dir.clone(),
            no_logs: self.no_logs,
            working_directory: self.working_directory.clone(),
            max_turns: self.max_turns,
            limits: self.limits,
            allow_host_execution: self.allow_host_execution,
        }
    }
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
    fn start(
        &self,
        model: &Model,
        effort: ReasoningEffort,
        token: &sav::CancellationToken,
    ) -> Result<Worker> {
        crate::executor::check_cancelled(token)?;
        let budgets = model.resolve_budgets_cancellable(
            self.context_limit,
            self.response_reserve,
            self.limits,
            token,
        )?;
        crate::executor::check_cancelled(token)?;
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
                SessionTranscript::open(&dir, format!("skott-{}", model.id), Some(&model.id))
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
        crate::executor::check_cancelled(token)?;
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
                "Context: {} tokens ({}); output: {} ({})",
                budgets.context_limit,
                budgets.context_source,
                budgets.limits.response_reserve,
                budgets.response_source,
            ),
        })
    }
}

struct TerminalRestore(bool);

impl TerminalRestore {
    fn restore(&mut self) -> std::io::Result<()> {
        self.0 = false;
        let raw = disable_raw_mode();
        let screen = execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            crossterm::cursor::Show
        );
        raw.and(screen)
    }
}

impl Drop for TerminalRestore {
    fn drop(&mut self) {
        if self.0
            && let Err(error) = self.restore()
        {
            tracing::error!(%error, "terminal restoration failed");
        }
    }
}

fn ui_loop(mut app: App, config: &Config, mut discovery: Discovery) -> Result<()> {
    enable_raw_mode()?;
    let mut terminal_restore = TerminalRestore(true);
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut worker: Option<Worker> = None;
    let mut bootstrap: Option<Bootstrap> = None;
    let mut builder: Option<SessionBuilder> = None;

    let result = run_ui(
        &mut terminal,
        &mut app,
        config,
        &mut discovery,
        &mut builder,
        &mut worker,
        &mut bootstrap,
    );

    discovery.cancel();
    if let Some(job) = &bootstrap {
        job.job.cancel();
    }
    if let Some(current) = &worker {
        current.handle.shutdown();
    }
    let restore = terminal_restore.restore();
    if let Err(error) = &restore {
        tracing::error!(%error, "terminal restoration failed");
    }
    result.and(restore.map_err(Error::from))
}

fn run_ui<M: std::io::Write>(
    terminal: &mut Terminal<CrosstermBackend<M>>,
    app: &mut App,
    config: &Config,
    discovery: &mut Discovery,
    builder: &mut Option<SessionBuilder>,
    worker: &mut Option<Worker>,
    bootstrap: &mut Option<Bootstrap>,
) -> Result<()> {
    // A prompt held while the session worker is starting (or being replaced
    // after a model/effort change or a new session); dispatched to the worker
    // as soon as it is installed, so a prompt never waits on the UI.
    let mut pending_submit = std::collections::VecDeque::new();
    let mut start_requested = true;
    // Automatic prompt replay: when a bootstrap (model load/switch) fails and a
    // prompt is held for it, the bootstrap is retried (up to this many times) so
    // the prompt is dispatched to a fresh worker once the model becomes ready —
    // the prompt is NOT lost and needs no manual resubmit. The model selection
    // is unchanged: a load/switch is retried, not a different model chosen.
    let mut replay_attempts: u32 = 0;
    const MAX_PROMPT_REPLAYS: u32 = 3;

    // Dispatch any prefilled, auto-started prompt (for example one supplied by
    // `kvist prompt`) through the same hold/send path, and clear the input so
    // the same text cannot be submitted again.
    if let Some(text) = app.take_pending_prompt() {
        app.editor.clear();
        pending_submit.push_back(text);
    }

    loop {
        terminal.draw(|frame| render::render(frame, app))?;

        if let Some(result) = discovery.model.as_ref().and_then(Job::poll) {
            discovery.model = None;
            match result {
                Ok(selection) => apply_selection(app, selection),
                Err(error) => {
                    app.model_discovery_pending = false;
                    app.push_event(crate::session::Event::Failed(error.describe()));
                }
            }
            if app.model.is_none() {
                start_requested = !pending_submit.is_empty();
                app.status = "choose a model (Tab)".into();
            }
        }
        if let Some(result) = discovery.tools.as_ref().and_then(Job::poll) {
            discovery.tools = None;
            match result {
                Ok(prepared) => {
                    for note in &prepared.summary_notes {
                        app.push_event(crate::session::Event::Note(note.clone()));
                    }
                    *builder = Some(prepared);
                }
                Err(error) => {
                    start_requested = false;
                    app.push_event(crate::session::Event::Failed(error.describe()));
                }
            }
        }
        if start_requested
            && worker.is_none()
            && bootstrap.is_none()
            && let Some(prepared) = builder.as_ref()
            && let Some(model) = app.model.as_deref().and_then(|id| config.model(id))
        {
            *bootstrap = Some(spawn_bootstrap(prepared, model, app.effort)?);
            start_requested = false;
        }

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

        // Install a finished session worker (initial start, a model/effort
        // change, or a new session) and dispatch any prompt held for it. A
        // failed bootstrap is reported, not fatal: the app stays usable, and the
        // next submit or new session starts a fresh bootstrap.
        if worker.is_none()
            && let Some(result) = bootstrap
                .as_ref()
                .and_then(|bootstrap| bootstrap.job.poll())
        {
            match result {
                Ok(new_worker) => {
                    *bootstrap = None;
                    if app.model.as_deref() != Some(&new_worker.model_id)
                        || app.effort != new_worker.effort
                    {
                        new_worker.handle.shutdown();
                        start_requested = true;
                        continue;
                    }
                    app.push_event(crate::session::Event::Note(new_worker.budget_note.clone()));
                    *worker = Some(new_worker);
                    if let Some(current) = worker.as_ref() {
                        for text in pending_submit.drain(..) {
                            current
                                .prompt_tx
                                .send(text)
                                .map_err(|_| Error::ChannelClosed)?;
                        }
                    }
                    replay_attempts = 0;
                    app.status = "ready".into();
                }
                Err(error) => {
                    // A failed bootstrap is reported, not fatal: the app stays
                    // usable. When a prompt is held for the failed worker, the
                    // bootstrap is replayed (up to a few attempts)
                    // so the prompt is dispatched to a fresh worker once the
                    // model becomes ready — the prompt is NOT lost and does not
                    // need a manual resubmit. The model selection is unchanged
                    // (a load/switch is retried, not a different model chosen).
                    *bootstrap = None;
                    app.push_event(crate::session::Event::Failed(error.describe()));
                    if !pending_submit.is_empty() {
                        if replay_attempts < MAX_PROMPT_REPLAYS {
                            replay_attempts += 1;
                            app.push_event(crate::session::Event::Note(format!(
                                "the model did not become ready in time; replaying the prompt \
                                     (attempt {replay_attempts}/{MAX_PROMPT_REPLAYS}) — the \
                                     prompt is dispatched once the model is ready"
                            )));
                            start_requested = true;
                        } else {
                            app.push_event(crate::session::Event::Note(
                                "the model did not become ready in time; the prompt is kept — \
                                 press Ctrl+Enter to replay it"
                                    .to_owned(),
                            ));
                        }
                    }
                }
            }
        }
        startup::update_status(
            app,
            worker.is_some(),
            bootstrap.is_some(),
            discovery.tools.is_some() || discovery.model.is_some(),
        );

        if event::poll(Duration::from_millis(30))? {
            match event::read()? {
                CrosstermEvent::Resize(width, height) => app.resize(width, height),
                CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                    match app.on_key(key) {
                        KeyAction::Submit => {
                            if !pending_submit.is_empty() {
                                replay_attempts = 0;
                                start_requested = true;
                            }
                            let text = app.take_pending_prompt();
                            if let Some(text) = text {
                                replay_attempts = 0;
                                if let Some(current) = worker.as_ref()
                                    && app.model.as_deref() == Some(&current.model_id)
                                    && app.effort == current.effort
                                {
                                    current
                                        .prompt_tx
                                        .send(text)
                                        .map_err(|_| Error::ChannelClosed)?;
                                    continue;
                                }
                                if let Some(old) = worker.take() {
                                    old.handle.shutdown();
                                }
                                if bootstrap.as_ref().is_some_and(|current| {
                                    app.model.as_deref() != Some(&current.model_id)
                                        || app.effort != current.effort
                                }) {
                                    *bootstrap = None;
                                }
                                pending_submit.push_back(text);
                                start_requested = true;
                            }
                        }
                        KeyAction::Cancel => {
                            if let Some(current) = worker.as_ref() {
                                current.handle.cancel();
                            }
                            if bootstrap.is_some() || !pending_submit.is_empty() {
                                *bootstrap = None;
                                pending_submit.clear();
                                start_requested = false;
                                app.status = "cancelled".into();
                            }
                        }
                        KeyAction::NewSession => {
                            // A new session needs a selected model; deferred
                            // (no model) sessions just note that.
                            if app.model.is_none() {
                                app.push_event(crate::session::Event::Note(
                                    "no model selected — press Tab to choose one before \
                                         starting a new session"
                                        .to_owned(),
                                ));
                                continue;
                            }
                            // Replace whatever is live (a running turn is
                            // cancelled with the old worker) and start a fresh
                            // session for the selected model and effort.
                            if let Some(old) = worker.take() {
                                old.handle.shutdown();
                            }
                            *bootstrap = None;
                            pending_submit.clear();
                            replay_attempts = 0;
                            start_requested = true;
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
