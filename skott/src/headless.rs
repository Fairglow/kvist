//! Terminal-free sandboxed workspace execution. This is not the engine broker.

use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use sav::CancellationToken;
use serde::Serialize;
use serde_json::json;

use crate::config::Config;
use crate::error::{Error, Result, io_error};
use crate::executor::SandboxExecutor;
use crate::session::{AgentRunner, AgentSession, Event, EventSink, RunSummary};
use crate::session_log::SessionLog;
use crate::tools::ToolRegistry;
use crate::tui::Overrides;

struct Output {
    writer: std::io::Stdout,
    sequence: u64,
}

struct OutputSink {
    output: Mutex<Output>,
    json: bool,
}

impl OutputSink {
    fn emit<T: Serialize>(&self, event: &T) -> Result<()> {
        let mut output = self.output.lock().map_err(|_| Error::Recording {
            reason: "output lock poisoned".into(),
        })?;
        output.sequence += 1;
        let envelope = json!({"schema_version":1, "sequence":output.sequence, "event":event});
        serde_json::to_writer(&mut output.writer, &envelope).map_err(|error| Error::Recording {
            reason: format!("write event stream: {error}"),
        })?;
        output.writer.write_all(b"\n")?;
        output.writer.flush()?;
        Ok(())
    }
}

impl EventSink for OutputSink {
    fn send(&self, event: Event) -> Result<()> {
        if self.json {
            self.emit(&event)
        } else {
            if let Event::Note(text) = event {
                eprintln!("{}", crate::error::terminal_text(&text));
            }
            Ok(())
        }
    }
}

fn state_directory() -> Result<PathBuf> {
    let base = if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        PathBuf::from(state)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/state")
    } else {
        return Err(Error::Config {
            path: None,
            reason: "headless logging needs --log-dir, XDG_STATE_HOME or HOME".into(),
        });
    };
    if !base.is_absolute() {
        return Err(Error::Config {
            path: None,
            reason: "headless state directory must be absolute".into(),
        });
    }
    Ok(base.join("agent-runner/runs"))
}

fn log_scope(log_dir: &Path, workdir: &Path) -> Result<PathBuf> {
    let absolute = if log_dir.is_absolute() {
        log_dir.to_owned()
    } else {
        std::env::current_dir()?.join(log_dir)
    };
    if absolute
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(Error::Config {
            path: None,
            reason: "log path must be canonical and contain no . or ..".into(),
        });
    }
    if absolute.starts_with(workdir) {
        return Err(Error::Config {
            path: None,
            reason: "headless logs must be outside the sandbox writable workspace".into(),
        });
    }
    Ok(absolute)
}

/// Executes one prompt with mandatory private recording and sandbox-only tools.
///
/// JSON writes ordered version-one NDJSON; plain mode writes only a successful
/// final answer. Startup failures use diagnostics rather than success events.
pub fn run(config: Config, overrides: Overrides, json_output: bool) -> Result<RunSummary> {
    if overrides.allow_host_execution || overrides.host_turns.is_some() || overrides.no_logs {
        return Err(Error::Config {
            path: None,
            reason: "headless execution forbids host execution and disabled recording".into(),
        });
    }
    overrides.limits.validate()?;
    let prompt = overrides.prompt.as_deref().ok_or_else(|| Error::Config {
        path: None,
        reason: "headless execution requires a prompt".into(),
    })?;
    if prompt.trim().is_empty() || prompt.len() > 64 * 1024 {
        return Err(Error::Config {
            path: None,
            reason: "prompt must be nonblank and at most 64 KiB".into(),
        });
    }
    let workdir = crate::config::resolve_working_directory(
        overrides
            .cwd
            .as_deref()
            .unwrap_or(&config.working_directory),
    )?;
    let logs = log_scope(
        &match &overrides.log_dir {
            Some(path) => path.clone(),
            None => state_directory()?,
        },
        &workdir,
    )?;
    // Active-model-first selection (headless): an explicit --model wins, then the
    // default provider's already-loaded model (no switch), then a single
    // default-provider model, then the default provider's default model. The
    // active probe is bounded, read-only, and loopback-only; a down provider
    // yields no entry and falls through.
    let active = crate::config::probe_active_models(&config);
    let selection =
        crate::config::select_active_model(&config, overrides.model.as_deref(), &active);
    let model_id = match &selection {
        crate::config::Select::Explicit(id) => id.clone(),
        crate::config::Select::Active {
            model_id, reason, ..
        } => {
            eprintln!("{}", crate::error::terminal_text(reason));
            model_id.clone()
        }
        crate::config::Select::Single { model_id } => model_id.clone(),
        crate::config::Select::Default {
            model_id, reason, ..
        } => {
            eprintln!("{}", crate::error::terminal_text(reason));
            model_id.clone()
        }
        // No active match and no single default model to fall back to: headless
        // cannot ask the user, so it fails fast before provider inference with
        // an actionable diagnostic.
        crate::config::Select::NeedsSelection => {
            let default_models: Vec<String> = config
                .default_provider_models()
                .iter()
                .map(|model| model.id.clone())
                .collect();
            let active_note: Vec<String> = active
                .iter()
                .map(|(base_url, loaded)| format!("`{loaded}` at {base_url}"))
                .collect();
            let mut reason = format!(
                "no active model and no unambiguous default to fall back to: {} configured \
                 model(s) for provider `{}` ({})",
                default_models.len(),
                config.default_provider,
                default_models.join(", ")
            );
            if !active_note.is_empty() {
                reason.push_str(&format!(
                    "; active provider model(s) found: {} — add a [[models]] entry for one",
                    active_note.join(", ")
                ));
            }
            reason.push_str(
                "; pass --model <id> to choose one, or mark one of the default provider's \
                 [[models]] with is_default = true",
            );
            return Err(Error::Config { path: None, reason });
        }
    };
    let model = config
        .model(&model_id)
        .ok_or_else(|| Error::ModelNotFound {
            requested: model_id,
            available: config.models.iter().map(|model| model.id.clone()).collect(),
        })?;
    let budgets = model.resolve_budgets(
        overrides.context_limit,
        overrides.response_reserve,
        overrides.limits,
    )?;
    let budget_note = format!(
        "model budget: {} context tokens ({}), {} output tokens ({})",
        budgets.context_limit,
        budgets.context_source,
        budgets.limits.response_reserve,
        budgets.response_source
    );
    let transport = model.transport()?;
    let registry = ToolRegistry::resolve_for_workspace(
        config.tool_policy.clone(),
        &config.tool_profiles,
        &crate::toolchain::HostProbe,
        overrides.profile,
        &workdir,
    )?;
    let resource_notes = registry.diagnostics().to_vec();
    let definitions = registry.tool_definitions();
    let executor = SandboxExecutor::new(registry, config.sandbox.clone(), workdir.clone());
    let mut log = SessionLog::open(&logs, format!("agent-runner-{}", model.id))
        .map_err(|source| {
            io_error(
                "open private headless journal",
                Some(&logs.to_string_lossy()),
                source,
            )
        })?
        .with_metadata(crate::session_log::SessionMetadata {
            execution_scope: crate::session_log::ExecutionScope::SandboxedWorkspace,
            working_directory: workdir.clone(),
            write_root: config.tool_policy.write_root.clone(),
            policy_identity: config.tool_policy.identity(),
            context_limit: budgets.context_limit,
            context_source: budgets.context_source.into(),
            response_reserve: budgets.limits.response_reserve,
            response_source: budgets.response_source.into(),
            max_run_tokens: overrides.limits.max_tokens,
            max_run_secs: overrides.limits.wall_time.as_secs(),
        });
    let mut session = AgentSession::new(
        model.clone(),
        overrides.effort.unwrap_or(config.default_thinking_effort),
        definitions,
        crate::run::system_prompt(&config.tool_policy.write_root),
    );
    for note in &resource_notes {
        crate::session::Recorder::notice(&mut log, note)?;
        eprintln!("{}", crate::error::terminal_text(note));
    }
    crate::session::Recorder::on_prompt(&mut log, prompt)?;
    session.push_user(prompt);
    let mut context = crate::context::ContextManager::for_model(budgets.context_limit);
    let sink = OutputSink {
        output: Mutex::new(Output {
            writer: std::io::stdout(),
            sequence: 0,
        }),
        json: json_output,
    };
    if json_output {
        sink.emit(&json!({"type":"run_start", "data":{
            "execution_scope":"sandboxed_workspace", "canonical_evidence":false,
            "working_directory":workdir, "policy_identity":config.tool_policy.identity(),
            "model":model.id, "context_limit":context.limit_tokens(),
            "context_source":budgets.context_source,
            "response_reserve":budgets.limits.response_reserve,
            "response_source":budgets.response_source,
            "max_run_tokens":overrides.limits.max_tokens,
            "max_run_secs":overrides.limits.wall_time.as_secs(),
        }}))?;
    }
    sink.send(Event::Note(budget_note))?;
    for note in resource_notes {
        if json_output {
            sink.send(Event::Note(note))?;
        }
    }
    sav::install_handler();
    let result = AgentRunner::with_retry(crate::session::MAX_TURNS, model.retry_policy())
        .with_limits(budgets.limits)?
        .run(
            &mut session,
            &transport,
            &executor,
            &sink,
            &CancellationToken::new(),
            &mut context,
            Some(&mut log),
        );
    let summary = match &result {
        Ok(summary) => summary.clone(),
        Err(error) => {
            sink.send(Event::Failed(error.describe()))?;
            RunSummary {
                failure: Some(error.describe()),
                ..RunSummary::default()
            }
        }
    };
    if json_output {
        let mut value = serde_json::to_value(&summary).map_err(|error| Error::Recording {
            reason: error.to_string(),
        })?;
        value["disposition"] = json!(summary.disposition());
        sink.emit(&json!({"type":"run_summary", "data":value}))?;
    } else if summary.success()
        && let Some(answer) = &summary.answer
    {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(crate::error::terminal_text(answer).as_bytes())?;
        stdout.flush()?;
    }
    result
}
