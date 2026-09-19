//! The terminal UI: setup, event loop, and teardown.

mod app;
mod render;

use std::io::IsTerminal;
use std::process::ExitCode;
use std::time::Duration;

use agent_runtime::ReasoningEffort;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event as CrosstermEvent, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode, size,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use self::app::App;
use crate::config::{Config, Model};
use crate::context::ContextManager;
use crate::error::{Error, Result};
use crate::executor::SandboxExecutor;
use crate::run::{self, start};
use crate::session::{AgentSession, Recorder};
use crate::session_log::SessionLog;
use crate::tools::ToolRegistry;

/// User-selected overrides applied over a loaded configuration.
pub struct Overrides {
    /// The model id to select for this session.
    pub model: Option<String>,
    /// The thinking effort to select for this session.
    pub effort: Option<ReasoningEffort>,
    /// The working directory to select for this session.
    pub cwd: Option<std::path::PathBuf>,
    /// The language tool profile to select for this session.
    pub profile: Option<crate::tools::ToolProfile>,
    /// Directory for the session journal + transcript. `None` means the
    /// default (`.agent-runner/runs` under the working directory).
    pub log_dir: Option<std::path::PathBuf>,
    /// Model context window in tokens. `None` uses the default (8192).
    pub context_limit: Option<usize>,
    /// Disable durable session logging entirely.
    pub no_logs: bool,
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

    let mut registry = match ToolRegistry::discover(config.tool_policy.clone()) {
        Ok(registry) => registry,
        Err(error) => {
            eprintln!("{}", error.describe());
            return ExitCode::from(error.exit_code());
        }
    };
    if let Some(profile) = overrides.profile {
        registry = registry.with_profiles(vec![profile]);
    }
    let tool_defs = registry.tool_definitions();

    let executor =
        SandboxExecutor::new(registry, config.sandbox.clone(), working_directory.clone());

    let transport = match build_transport(&model) {
        Ok(transport) => transport,
        Err(error) => {
            eprintln!("{}", error.describe());
            return ExitCode::from(error.exit_code());
        }
    };

    let session_label = app_model_id(&config, &model);
    let session = AgentSession::new(
        model,
        effort,
        tool_defs,
        run::system_prompt(&config.tool_policy.write_root),
    );

    // Bound the model context via automatic compaction, and open a durable log
    // (journal + readable transcript, including reasoning) unless disabled.
    let context = ContextManager::new(
        overrides
            .context_limit
            .unwrap_or(crate::context::DEFAULT_CONTEXT_TOKENS),
        6,
    );
    let recorder = if overrides.no_logs {
        None
    } else {
        let dir = overrides
            .log_dir
            .clone()
            .unwrap_or_else(|| working_directory.join(crate::session_log::DEFAULT_LOG_DIR));
        match SessionLog::open(&dir, format!("agent-runner-{}", session_label)) {
            Ok(log) => Some(Box::new(log) as Box<dyn Recorder>),
            Err(error) => {
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

    let (handle, rx, prompt_tx) = start(session, transport, executor, context, recorder);

    let (width, height) = size().unwrap_or((100, 30));
    let mut app = App::new(&app_model_label, &config.effort_label(), width, height);
    app.handle = Some(handle);
    app.rx = Some(rx);
    app.prompt_tx = Some(prompt_tx);

    match ui_loop(app) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.describe());
            ExitCode::from(error.exit_code())
        }
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
    agent_runtime::DirectModelTransport::new(provider, &model.base_url, deadline, 8 * 1024 * 1024)
        .map_err(|error| Error::ModelTransport {
            model: Some(model.id.clone()),
            reason: error.to_string(),
        })
}

/// Trait shim so the UI can display the current effort as a label.
trait EffortLabel {
    fn effort_label(&self) -> String;
}

impl EffortLabel for Config {
    fn effort_label(&self) -> String {
        format!("{:?}", self.default_thinking_effort)
    }
}

fn ui_loop(mut app: App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_ui(&mut terminal, &mut app);

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
) -> Result<()> {
    loop {
        terminal.draw(|frame| render::render(frame, app))?;

        // Drain loop events without blocking.
        if app.pump()? {
            // nothing else to do this iteration
        }

        if event::poll(Duration::from_millis(150))? {
            match event::read()? {
                CrosstermEvent::Resize(width, height) => app.resize(width, height),
                CrosstermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                    app.on_key(key);
                }
                CrosstermEvent::Key(key) if key.kind == KeyEventKind::Release => {}
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
